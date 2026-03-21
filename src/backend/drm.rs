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
        renderer::glow::GlowRenderer,
        session::{Event as SessionEvent, Session, libseat::LibSeatSession},
        udev::{UdevBackend, UdevEvent, all_gpus, primary_gpu},
    },
    desktop::{layer_map_for_output, utils::send_frames_surface_tree},
    output::{Output, OutputModeSource, PhysicalProperties, Subpixel},
    reexports::{
        calloop::{EventLoop, LoopHandle},
        drm::control::{self, Device as ControlDevice, ModeTypeFlags, connector, crtc},
        input::{Device, Libinput},
        rustix::fs::OFlags,
    },
    utils::DeviceFd,
    wayland::{dmabuf::DmabufFeedbackBuilder, image_copy_capture::DmabufConstraints},
};

use smithay_drm_extras::{
    display_info,
    drm_scanner::{DrmScanEvent, DrmScanner},
};

use tracing::{error, info, warn};

use crate::{
    Monotile,
    input::configure_device,
    render::{Shaders, send_frame_callbacks},
    shell::{MonitorSettings, Monitors},
    state::State,
};

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
    pub compositor: Surface,
    pub render: RenderState,
    pub connector: connector::Handle,
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
    pub dma_constraints: Option<DmabufConstraints>,
    pub surfaces: HashMap<crtc::Handle, OutputSurface>,
    pub loop_handle: LoopHandle<'static, Monotile>,
    pub input_devices: Vec<Device>,
    libinput: Libinput,
    scanner: DrmScanner,
}

impl std::fmt::Debug for DrmState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DrmState").finish_non_exhaustive()
    }
}

impl Drop for DrmState {
    fn drop(&mut self) {
        self.drm.pause();
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
        if !self.session.is_active() {
            surface.render = RenderState::Idle;
            return;
        }
        surface.render = RenderState::Idle;
        let Some((_, mon)) = state.monitors.by_output(&surface.output) else {
            return;
        };

        // skip frame if a window has a pending resize (no flicker)
        if !state.locked && state.windows.any_pending_resize(mon.tag()) {
            send_frame_callbacks(
                state.windows.visible(mon.tag()),
                &surface.output,
                state.start_time.elapsed(),
                &mut state.popups,
            );
            self.schedule_render_crtc(crtc);
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
            &state.config,
            state.locked,
        ));

        let result = match surface.compositor.render_frame(
            &mut self.renderer,
            &elems,
            mon.settings.background,
            FrameFlags::DEFAULT,
        ) {
            Ok(result) => result,
            Err(err) => {
                warn!(?err, "failed to render frame");
                return;
            }
        };

        // capture pending screencopy frames for this output
        if !state.screencopy.pending.is_empty() {
            crate::handlers::screencopy::capture_output(
                &mut self.renderer,
                &mut state.screencopy,
                &surface.output,
                &elems,
                mon.settings.background,
                state.start_time.elapsed(),
            );
        }

        if result.is_empty {
            send_frame_callbacks(
                state.windows.visible(mon.tag()),
                &surface.output,
                state.start_time.elapsed(),
                &mut state.popups,
            );
            return;
        }

        if let Err(err) = surface.compositor.queue_frame(()) {
            warn!(?err, "failed to queue frame");
            return;
        }
        surface.render = RenderState::WaitingForVBlank;

        if let Some(ls) = &mon.lock_surface {
            send_frames_surface_tree(
                ls.wl_surface(),
                &surface.output,
                state.start_time.elapsed(),
                None,
                |_, _| Some(surface.output.clone()),
            );
        }
        send_frame_callbacks(
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

    pub fn apply_output_settings(&mut self, monitors: &Monitors) {
        let DrmState { surfaces, drm, .. } = self;
        for surface in surfaces.values_mut() {
            let Some((_, mon)) = monitors.by_output(&surface.output) else {
                continue;
            };
            let settings = &mon.settings;
            let output = &surface.output;

            // get connector modes, resolve and diff against pending_mode.
            if let Some(Err(err)) = drm
                .get_connector(surface.connector, false)
                .ok()
                .and_then(|c| drm_mode_for_config(c.modes(), settings))
                .filter(|(_, sel)| surface.compositor.pending_mode() != *sel)
                .map(|(_, sel)| surface.compositor.use_mode(sel))
            {
                warn!(?err, "failed to set mode on {}", output.name());
            }

            // diff and change output state only if necessary
            let mode = surface.compositor.pending_mode().into();
            let transform = settings.transform.unwrap_or(output.current_transform());
            let scale = settings.scale.unwrap_or(output.current_scale());

            let changed = output.current_mode() != Some(mode)
                || output.current_transform() != transform
                || output.current_scale().fractional_scale() != scale.fractional_scale()
                || output.current_location() != settings.pos;

            if changed {
                output.change_current_state(
                    Some(mode),
                    Some(transform),
                    Some(scale),
                    Some(settings.pos),
                );
                layer_map_for_output(&surface.output).arrange();
            }
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
    info!("'{name}': make={make} model={model} serial={serial}");

    let s = MonitorSettings::resolve(&state.config.outputs, &name, &make, &model, &serial);
    let Some((preferred, selected)) = drm_mode_for_config(connector.modes(), &s) else {
        warn!("connector {name} has no modes, skipping");
        return;
    };

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
    output.set_preferred(preferred.into());
    output.change_current_state(Some(selected.into()), s.transform, s.scale, Some(s.pos));

    let (mw, mh) = (selected.size().0, selected.size().1);
    info!("{}: {mw}x{mh}@{}Hz", output.name(), selected.vrefresh());

    let surface = match drm
        .drm
        .create_surface(crtc, selected, &[connector.handle()])
    {
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

    state.add_monitor(output.clone(), s);
    drm.surfaces.insert(
        crtc,
        OutputSurface {
            output,
            compositor,
            render: RenderState::default(),
            connector: connector.handle(),
        },
    );
    drm.schedule_render_crtc(crtc);
}

fn connector_disconnected(drm: &mut DrmState, state: &mut State, crtc: crtc::Handle) {
    let Some(surface) = drm.surfaces.remove(&crtc) else {
        return;
    };
    info!("{}: disconnected", surface.output.name());
    state.remove_monitor(&surface.output);
    if !state.monitors.is_empty() {
        drm.schedule_render(&state.monitors[state.active_monitor].output);
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
    let libinput_ctx = libinput.clone();

    let (render_node, card_node) = find_gpu(&seat)?;
    info!(
        "GPU: gpu={} card={}",
        render_node.dev_path().unwrap_or_default().display(),
        card_node.dev_path().unwrap_or_default().display(),
    );

    let fd = session.open(
        &card_node.dev_path().expect("no device path"),
        OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
    )?;
    let fd = DrmDeviceFd::new(DeviceFd::from(fd));
    let dev_id = fd.dev_id()?;
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
    let dma_constraints = {
        let mut formats = HashMap::<Fourcc, Vec<_>>::new();
        for f in render_formats.iter() {
            formats.entry(f.code).or_default().push(f.modifier);
        }
        Some(DmabufConstraints {
            node: render_node,
            formats: formats.into_iter().collect(),
        })
    };

    let feedback = DmabufFeedbackBuilder::new(dev_id, render_formats.iter().copied()).build()?;
    monotile.state.dmabuf_global = Some(
        monotile
            .state
            .dmabuf_state
            .create_global_with_default_feedback::<Monotile>(
                &monotile.state.display_handle,
                &feedback,
            ),
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
        dma_constraints,
        surfaces: HashMap::new(),
        loop_handle: loop_handle.clone(),
        input_devices: Vec::new(),
        libinput: libinput_ctx,
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
                drm.libinput.suspend();
                for surface in drm.surfaces.values_mut() {
                    surface.render = RenderState::Idle;
                }
                drm.drm.pause();
            }
            SessionEvent::ActivateSession => {
                info!("session activated");
                if let Err(()) = drm.libinput.resume() {
                    error!("failed to resume libinput");
                }
                if let Err(err) = drm.drm.activate(false) {
                    error!(?err, "failed to activate DRM");
                }
                for surface in drm.surfaces.values_mut() {
                    if let Err(err) = surface.compositor.reset_state() {
                        warn!(?err, "failed to reset compositor state");
                    }
                }
                for crtc in drm.surfaces.keys().copied().collect::<Vec<_>>() {
                    drm.schedule_render_crtc(crtc);
                }

                drm.loop_handle.insert_idle(|mt: &mut Monotile| {
                    device_changed(mt.backend.drm(), &mut mt.state);
                    mt.update_focus();
                });
            }
        }
    })?;

    device_changed(monotile.backend.drm(), &mut monotile.state);

    let udev = UdevBackend::new(&seat)?;
    loop_handle.insert_source(udev, |event, _, mt| {
        if let UdevEvent::Changed { .. } = event {
            device_changed(mt.backend.drm(), &mut mt.state);
            mt.update_focus();
        }
    })?;

    loop_handle.insert_source(LibinputInputBackend::new(libinput), |mut event, _, mt| {
        {
            let drm = mt.backend.drm();
            match &mut event {
                InputEvent::DeviceAdded { device } => {
                    configure_device(device, &mt.state.config);
                    drm.input_devices.push(device.clone());
                }
                InputEvent::DeviceRemoved { device } => {
                    drm.input_devices.retain(|d| d != device);
                }
                _ => {}
            }
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

fn drm_mode_for_config(
    modes: &[control::Mode],
    s: &MonitorSettings,
) -> Option<(control::Mode, control::Mode)> {
    let mut preferred = None;
    let mut matching: Option<control::Mode> = None;
    for &mode in modes {
        if mode.mode_type().contains(ModeTypeFlags::PREFERRED) {
            preferred = Some(mode);
        }
        if let Some(requested) = s.mode
            && mode.size() == requested.size
        {
            match requested.refresh {
                Some(hz) if mode.vrefresh() == hz => matching = Some(mode),
                None if matching.is_none_or(|b| mode.vrefresh() > b.vrefresh()) => {
                    matching = Some(mode)
                }
                _ => {}
            }
        }
    }
    let preferred = preferred.or(modes.first().copied())?;
    Some((preferred, matching.unwrap_or(preferred)))
}
