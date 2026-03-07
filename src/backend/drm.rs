// SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashMap;

use smithay::{
    backend::{
        allocator::{
            Fourcc,
            format::FormatSet,
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
        },
        drm::{
            DrmDevice, DrmDeviceFd, DrmEvent, DrmNode, NodeType,
            compositor::{DrmCompositor, FrameFlags},
            exporter::gbm::GbmFramebufferExporter,
        },
        egl::{EGLContext, EGLDisplay},
        input::InputEvent,
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{ImportDma, glow::GlowRenderer},
        session::{Event as SessionEvent, Session, libseat::LibSeatSession},
        udev::{UdevBackend, UdevEvent, all_gpus, primary_gpu},
    },
    output::{Output, OutputModeSource, PhysicalProperties, Subpixel},
    reexports::{
        calloop::{EventLoop, LoopHandle},
        drm::control::{ModeTypeFlags, connector, crtc},
        input::Libinput,
        rustix::fs::OFlags,
        wayland_server::backend::GlobalId,
    },
    utils::DeviceFd,
};

use smithay_drm_extras::{
    display_info,
    drm_scanner::{DrmScanEvent, DrmScanner},
};

use tracing::{error, info, warn};

use crate::{Monotile, render::Shaders, render::send_frame_callbacks, state::State};

type Allocator = GbmAllocator<DrmDeviceFd>;
type Exporter = GbmFramebufferExporter<DrmDeviceFd>;
type Surface = DrmCompositor<Allocator, Exporter, (), DrmDeviceFd>;

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderState {
    #[default]
    Idle,
    Queued,
    WaitingForVBlank,
}

pub struct OutputSurface {
    pub output: Output,
    pub global: GlobalId,
    pub compositor: Surface,
    pub render: RenderState,
}

pub struct DrmState {
    pub renderer: GlowRenderer,
    pub session: LibSeatSession,
    pub shaders: Shaders,
    pub gbm: GbmDevice<DrmDeviceFd>,
    pub drm: DrmDevice,
    pub allocator: Allocator,
    pub exporter: Exporter,
    pub render_formats: FormatSet,
    pub surfaces: HashMap<crtc::Handle, OutputSurface>,
    pub loop_handle: LoopHandle<'static, Monotile>,
    scanner: DrmScanner,
}

impl std::fmt::Debug for DrmState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DrmState").finish_non_exhaustive()
    }
}

impl DrmState {
    pub fn schedule_render(&mut self, output: &Output) {
        let crtc = self
            .surfaces
            .iter()
            .find(|(_, s)| s.output == *output)
            .map(|(&c, _)| c);
        if let Some(crtc) = crtc {
            self.schedule_render_crtc(crtc);
        }
    }

    fn schedule_render_crtc(&mut self, crtc: crtc::Handle) {
        let Some(surface) = self.surfaces.get_mut(&crtc) else {
            return;
        };
        match surface.render {
            RenderState::Idle => {
                surface.render = RenderState::Queued;
                self.loop_handle.insert_idle(move |mt: &mut Monotile| {
                    mt.backend.drm().render(crtc, &mut mt.state);
                });
            }
            RenderState::WaitingForVBlank => {
                surface.render = RenderState::Queued;
            }
            RenderState::Queued => {}
        }
    }

    pub fn render(&mut self, crtc: crtc::Handle, state: &mut State) {
        let Some(surface) = self.surfaces.get_mut(&crtc) else {
            return;
        };
        if surface.render != RenderState::Queued {
            return;
        }
        surface.render = RenderState::Idle;
        let Some(mon) = state.monitors.iter().find(|m| m.output == surface.output) else {
            return;
        };

        // skip frame if a tiled window has a pending resize (no flicker)
        if state.windows.any_pending_resize(mon.tag()) {
            send_frame_callbacks(
                state.windows.visible(mon.tag()),
                &surface.output,
                state.start_time.elapsed(),
                &mut state.popups,
            );
            return;
        }

        let ptr = state.seat.get_pointer().unwrap();
        let pos = ptr.current_location();
        let mut elems = state.cursor.elements(&mut self.renderer, pos);

        elems.extend(crate::render::output_elements(
            &mut self.renderer,
            mon,
            &state.windows,
            &self.shaders,
        ));

        let result = match surface.compositor.render_frame(
            &mut self.renderer,
            &elems,
            crate::config::BG_COLOR,
            FrameFlags::DEFAULT | FrameFlags::ALLOW_PRIMARY_PLANE_SCANOUT_ANY,
        ) {
            Ok(result) => result,
            Err(err) => {
                warn!(?err, "failed to render frame");
                return;
            }
        };

        if result.is_empty {
            return;
        }

        if let Err(err) = surface.compositor.queue_frame(()) {
            warn!(?err, "failed to queue frame");
            return;
        }
        surface.render = RenderState::WaitingForVBlank;

        crate::render::send_frame_callbacks(
            state.windows.visible(mon.tag()),
            &surface.output,
            state.start_time.elapsed(),
            &mut state.popups,
        );
    }

    pub fn frame_finish(&mut self, crtc: crtc::Handle) {
        let Some(surface) = self.surfaces.get_mut(&crtc) else {
            return;
        };
        if let Err(err) = surface.compositor.frame_submitted() {
            warn!(?err, "frame_submitted failed");
        }
        let redraw = surface.render == RenderState::Queued;
        surface.render = RenderState::Idle;
        if redraw {
            self.schedule_render_crtc(crtc);
        }
    }
}

pub fn device_changed(drm: &mut DrmState, state: &mut State) {
    let scan = match drm.scanner.scan_connectors(&drm.drm) {
        Ok(s) => s,
        Err(err) => {
            error!(?err, "connector scan failed");
            return;
        }
    };

    for event in scan {
        match event {
            DrmScanEvent::Connected {
                connector,
                crtc: Some(crtc),
            } => connector_connected(drm, state, connector, crtc),
            DrmScanEvent::Disconnected {
                crtc: Some(crtc), ..
            } => connector_disconnected(drm, state, crtc),
            _ => {}
        }
    }
}

fn connector_connected(
    drm: &mut DrmState,
    state: &mut State,
    connector: connector::Info,
    crtc: crtc::Handle,
) {
    let name = format!(
        "{}-{}",
        connector.interface().as_str(),
        connector.interface_id()
    );
    info!("Connected: {}", name);

    let di = display_info::for_connector(&drm.drm, connector.handle());
    let (make, model, serial) = if let Some(di) = &di {
        (
            di.make().unwrap_or_else(|| "Unknown".into()),
            di.model().unwrap_or_else(|| "Unknown".into()),
            di.serial().unwrap_or_else(|| "Unknown".into()),
        )
    } else {
        ("Unknown".into(), "Unknown".into(), "Unknown".into())
    };

    let Some(mode) = connector
        .modes()
        .iter()
        .find(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED))
        .or(connector.modes().first())
        .copied()
    else {
        warn!("connector {name} has no modes, skipping");
        return;
    };

    // TODO: output positioning
    let (output_w, output_h) = connector.size().unwrap_or((0, 0));
    let output = Output::new(
        name,
        PhysicalProperties {
            size: (output_w as i32, output_h as i32).into(),
            subpixel: Subpixel::Unknown,
            make,
            model,
            serial_number: serial,
        },
    );
    let wl_mode = mode.into();
    output.change_current_state(Some(wl_mode), None, None, Some((0, 0).into()));
    output.set_preferred(wl_mode);

    let surface = match drm.drm.create_surface(crtc, mode, &[connector.handle()]) {
        Ok(s) => s,
        Err(err) => {
            warn!(?err, "Failed to create DRM surface");
            return;
        }
    };

    let compositor = match DrmCompositor::new(
        OutputModeSource::Auto(output.clone()),
        surface,
        None,
        drm.allocator.clone(),
        drm.exporter.clone(),
        [Fourcc::Xrgb8888, Fourcc::Argb8888],
        drm.render_formats.clone(),
        drm.drm.cursor_size(),
        Some(drm.gbm.clone()),
    ) {
        Ok(c) => c,
        Err(err) => {
            warn!(?err, "Failed to initialize DRM compositor");
            return;
        }
    };

    let global = output.create_global::<Monotile>(&state.display_handle);
    state.add_monitor(output.clone());
    drm.surfaces.insert(
        crtc,
        OutputSurface {
            output,
            global,
            compositor,
            render: RenderState::default(),
        },
    );
    drm.render(crtc, state);
}

fn connector_disconnected(drm: &mut DrmState, state: &mut State, crtc: crtc::Handle) {
    if let Some(surface) = drm.surfaces.remove(&crtc) {
        info!("Disconnected: {}", surface.output.name());
        state.monitors.retain(|m| m.output != surface.output);
        if state.active_monitor >= state.monitors.len() && !state.monitors.is_empty() {
            state.active_monitor = state.monitors.len() - 1;
        }
    }
}

pub fn init(
    event_loop: &mut EventLoop<'static, Monotile>,
    monotile: &mut Monotile,
) -> Result<(), Box<dyn std::error::Error>> {
    let loop_handle = event_loop.handle();
    let (mut session, session_notifier) = LibSeatSession::new()?;
    let seat = session.seat();
    let mut libinput = Libinput::new_with_udev(LibinputSessionInterface::from(session.clone()));
    libinput.udev_assign_seat(&seat).unwrap();

    let (render_node, card_node) = find_gpu(&seat)?;
    info!(
        "selected GPU: gpu={} card={}",
        render_node.dev_path().unwrap_or_default().display(),
        card_node.dev_path().unwrap_or_default().display(),
    );

    let fd = session.open(
        &card_node.dev_path().expect("no device path"),
        OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
    )?;
    let fd = DrmDeviceFd::new(DeviceFd::from(fd));
    let (drm, drm_notifier) = DrmDevice::new(fd.clone(), false)?;
    let gbm = GbmDevice::new(fd)?;
    let egl_display = unsafe { EGLDisplay::new(gbm.clone()) }?;
    let egl_context = EGLContext::new(&egl_display)?;
    let mut renderer = unsafe { GlowRenderer::new(egl_context) }?;
    let shaders = crate::render::compile_shaders(&mut renderer);

    let allocator = GbmAllocator::new(
        gbm.clone(),
        GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
    );
    let exporter = GbmFramebufferExporter::new(gbm.clone(), render_node.into());
    let render_formats = renderer.egl_context().dmabuf_render_formats().clone();

    monotile.state.dmabuf_global = Some(
        monotile
            .state
            .dmabuf_state
            .create_global::<Monotile>(&monotile.state.display_handle, renderer.dmabuf_formats()),
    );

    monotile.backend = crate::backend::Backend::Drm(DrmState {
        renderer,
        session,
        shaders,
        gbm,
        drm,
        allocator,
        exporter,
        render_formats,
        surfaces: HashMap::new(),
        loop_handle: loop_handle.clone(),
        scanner: DrmScanner::new(),
    });

    loop_handle.insert_source(drm_notifier, |event, _, mt| match event {
        DrmEvent::VBlank(crtc) => mt.backend.drm().frame_finish(crtc),
        DrmEvent::Error(err) => error!(?err, "DRM error"),
    })?;

    loop_handle.insert_source(session_notifier, |event, _, mt| {
        let drm = mt.backend.drm();
        match event {
            SessionEvent::PauseSession => {
                info!("session paused");
                drm.drm.pause();
            }
            SessionEvent::ActivateSession => {
                info!("session activated");
                if let Err(err) = drm.drm.activate(false) {
                    error!(?err, "failed to activate DRM");
                }
                for crtc in drm.surfaces.keys().copied().collect::<Vec<_>>() {
                    drm.schedule_render_crtc(crtc);
                }
            }
        }
    })?;

    device_changed(monotile.backend.drm(), &mut monotile.state);

    let udev = UdevBackend::new(&seat)?;
    loop_handle.insert_source(udev, |event, _, mt| {
        if let UdevEvent::Changed { .. } = event {
            device_changed(mt.backend.drm(), &mut mt.state);
        }
    })?;

    loop_handle.insert_source(LibinputInputBackend::new(libinput), |mut event, _, mt| {
        if let InputEvent::DeviceAdded { ref mut device } = event {
            crate::input::configure_device(device);
        }
        mt.process_input_event(event);
    })?;

    Ok(())
}

fn find_gpu(seat: &str) -> Result<(DrmNode, DrmNode), Box<dyn std::error::Error>> {
    let render_node = if let Ok(var) = std::env::var("DRM_DEVICE") {
        DrmNode::from_path(var)?
    } else {
        primary_gpu(seat)?
            .and_then(|p| {
                DrmNode::from_path(p)
                    .ok()?
                    .node_with_type(NodeType::Render)?
                    .ok()
            })
            .unwrap_or_else(|| {
                all_gpus(seat)
                    .unwrap()
                    .into_iter()
                    .find_map(|p| DrmNode::from_path(p).ok())
                    .expect("No GPU!")
            })
    };
    let card_node = match render_node.node_with_type(NodeType::Primary) {
        Some(Ok(node)) => node,
        _ => render_node,
    };
    Ok((render_node, card_node))
}
