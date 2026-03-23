// SPDX-License-Identifier: GPL-3.0-only

mod layout;
mod monitor;
mod tag;
mod window;

pub use layout::TilingLayout;
pub use monitor::{Monitor, MonitorSettings, Monitors};
pub use tag::Tag;
pub use window::{Placement, Unmapped, WindowElement, Windows};

use slotmap::new_key_type;

new_key_type! {
    pub struct WindowId;
}
