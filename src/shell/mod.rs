// SPDX-License-Identifier: GPL-3.0-only

mod layout;
mod monitor;
mod tag;
mod transition;
mod view;
mod window;

pub use layout::TilingLayout;
pub use monitor::{Monitor, MonitorSettings, Monitors};
pub use tag::Tag;
pub use transition::{LayoutBlocker, LayoutTransition};
pub use view::{Tile, View};
pub use window::{Placement, ToplevelSurfaceExt, Unmapped, WindowElement, Windows};

use slotmap::new_key_type;

new_key_type! {
    pub struct WindowId;
}
