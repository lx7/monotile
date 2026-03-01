// SPDX-License-Identifier: GPL-3.0-only

use smithay::{
    backend::{
        allocator::{
            Fourcc,
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
        },
        drm::{
            DrmDevice, DrmDeviceFd, DrmEvent, DrmNode, NodeType,
            exporter::gbm::{GbmFramebufferExporter, NodeFilter},
            output::{DrmOutput, DrmOutputManager, DrmOutputRenderElements},
        },
        egl::{EGLContext, EGLDisplay},
        renderer::{ImportDma, glow::GlowRenderer},
        session::{Session, libseat::LibSeatSession},
        udev::{all_gpus, primary_gpu},
    },
    output::Output,
    reexports::{calloop::EventLoop, rustix::fs::OFlags},
    utils::DeviceFd,
    wayland::dmabuf::DmabufFeedbackBuilder,
};

use tracing::{debug, error, info, trace, warn};

use crate::Monotile;

pub struct DrmState {
    pub renderer: GlowRenderer,
    pub session: LibSeatSession,
    pub shaders: crate::render::Shaders,
    pub gbm: GbmDevice<DrmDeviceFd>,
    pub output_mgr: DrmOutputManager<
        GbmAllocator<DrmDeviceFd>,
        GbmFramebufferExporter<DrmDeviceFd>,
        (),
        DrmDeviceFd,
    >,
}

impl std::fmt::Debug for DrmState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DrmState").finish_non_exhaustive()
    }
}

impl DrmState {
    pub fn schedule_render(&self, _output: &Output) {}
}

pub fn init(
    event_loop: &mut EventLoop<Monotile>,
    monotile: &mut Monotile,
) -> Result<(), Box<dyn std::error::Error>> {
    let (mut session, _session_notifier) = LibSeatSession::new()?;

    let primary_gpu = if let Ok(var) = std::env::var("DRM_DEVICE") {
        DrmNode::from_path(var).expect("Invalid drm device path")
    } else {
        primary_gpu(session.seat())
            .unwrap()
            .and_then(|x| {
                DrmNode::from_path(x)
                    .ok()?
                    .node_with_type(NodeType::Render)?
                    .ok()
            })
            .unwrap_or_else(|| {
                all_gpus(session.seat())
                    .unwrap()
                    .into_iter()
                    .find_map(|x| DrmNode::from_path(x).ok())
                    .expect("No GPU!")
            })
    };
    info!("Primary GPU: {}", primary_gpu);

    let path = primary_gpu.dev_path().expect("no device path for GPU");
    let fd = session.open(
        &path,
        OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
    )?;
    let fd = DrmDeviceFd::new(DeviceFd::from(fd));

    // drm == kernel modesetting API
    let (drm, drm_notifier) = DrmDevice::new(fd.clone(), true)?;

    // gbm == GPU buffer mgmt
    let gbm = GbmDevice::new(fd)?;

    let egl_display = unsafe { EGLDisplay::new(gbm.clone()) }?;
    let egl_context = EGLContext::new(&egl_display)?;
    let mut renderer = unsafe { GlowRenderer::new(egl_context) }?;
    let shaders = crate::render::compile_shaders(&mut renderer);

    let allocator = GbmAllocator::new(
        gbm.clone(),
        GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
    );
    let exporter = GbmFramebufferExporter::new(gbm.clone(), primary_gpu.into());
    let render_formats = renderer.egl_context().dmabuf_render_formats().clone();

    let output_mgr = DrmOutputManager::new(
        drm,
        allocator,
        exporter,
        Some(gbm.clone()),
        [
            Fourcc::Abgr2101010,
            Fourcc::Argb2101010,
            Fourcc::Abgr8888,
            Fourcc::Argb8888,
        ],
        render_formats,
    );

    let dmabuf_formats = renderer.dmabuf_formats();
    let default_feedback = DmabufFeedbackBuilder::new(primary_gpu.dev_id(), dmabuf_formats)
        .build()
        .expect("failed to build dmabuf feedback");

    let dmabuf_global = monotile
        .state
        .dmabuf_state
        .create_global_with_default_feedback::<Monotile>(
            &monotile.state.display_handle,
            &default_feedback,
        );
    monotile.state.dmabuf_global = Some(dmabuf_global);

    monotile.backend = crate::backend::Backend::Drm(DrmState {
        renderer: renderer,
        session: session,
        shaders: shaders,
        gbm: gbm,
        output_mgr: output_mgr,
    });

    // use events from drm_notifier (vblank) to render the next frame
    event_loop
        .handle()
        .insert_source(drm_notifier, move |event, _, monotile| match event {
            DrmEvent::VBlank(_crtc) => {}
            DrmEvent::Error(err) => {
                error!(?err, "DRM error");
            }
        })?;
    Ok(())
}
