// SPDX-License-Identifier: GPL-3.0-only

use crate::Monotile;
use smithay::{
    backend::renderer::ImportDma,
    delegate_dmabuf,
    wayland::dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier},
};

impl DmabufHandler for Monotile {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.state.dmabuf_state
    }

    fn dmabuf_imported(
        &mut self,
        _global: &DmabufGlobal,
        dmabuf: smithay::backend::allocator::dmabuf::Dmabuf,
        notifier: ImportNotifier,
    ) {
        if self.backend.renderer().import_dmabuf(&dmabuf, None).is_ok() {
            let _ = notifier.successful::<Monotile>();
        } else {
            notifier.failed();
        }
    }
}
delegate_dmabuf!(Monotile);
