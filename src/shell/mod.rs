// SPDX-License-Identifier: GPL-3.0-only

mod blocker;
mod layout;
mod monitor;
mod tag;
mod view;
mod window;

pub use blocker::LayoutBlocker;
pub use layout::TilingLayout;
pub use monitor::{Monitor, MonitorSettings, Monitors};
pub use tag::Tag;
pub use view::{Tile, View, Views};
pub use window::{Placement, ToplevelSurfaceExt, Unmapped, WindowElement, Windows};

use slotmap::new_key_type;

new_key_type! {
    pub struct WindowId;
}
