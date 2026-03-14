// SPDX-License-Identifier: GPL-3.0-only
// Generated from protocols/monotile-ipc-unstable-v1.xml

#![allow(non_upper_case_globals, unused)]

pub mod __interfaces {
    use wayland_client::protocol::__interfaces::*;
    wayland_scanner::generate_interfaces!("protocols/monotile-ipc-unstable-v1.xml");
}

use self::__interfaces::*;
use wayland_client;
use wayland_client::protocol::*;

wayland_scanner::generate_client_code!("protocols/monotile-ipc-unstable-v1.xml");
