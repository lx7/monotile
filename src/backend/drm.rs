// SPDX-License-Identifier: GPL-3.0-only

use smithay::{
    backend::{
        allocator::{
            Fourcc,
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
        },
        drm::{DrmDevice, DrmDeviceFd, DrmNode, NodeType},
        egl::{EGLContext, EGLDisplay},
        renderer::glow::GlowRenderer,
        session::{Session, libseat::LibSeatSession},
        udev::{all_gpus, primary_gpu},
    },
    output::Output,
    reexports::{calloop::EventLoop, rustix::fs::OFlags},
    utils::DeviceFd,
};
use tracing::{debug, error, info, trace, warn};

use crate::Monotile;

pub struct DrmState {
    pub renderer: GlowRenderer,
    pub session: LibSeatSession,
    pub shaders: crate::render::Shaders,
    pub gbm: GbmDevice<DrmDeviceFd>,
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
    _event_loop: &mut EventLoop<Monotile>,
    _monotile: &mut Monotile,
) -> Result<(), Box<dyn std::error::Error>> {
    let (mut session, _session_notifier) = LibSeatSession::new()?;

    let gpu = if let Ok(var) = std::env::var("DRM_DEVICE") {
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
    info!("Primary GPU: {}", gpu);

    let path = gpu.dev_path().expect("no device path for GPU");
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

    let _state = DrmState {
        renderer: renderer,
        session: session,
        shaders: shaders,
        gbm: gbm,
    };
    Ok(())
}
