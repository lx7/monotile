// SPDX-License-Identifier: GPL-3.0-only
// Generated from protocols/dwl-ipc-unstable-v2.xml

#![allow(non_upper_case_globals, unused)]

pub mod __interfaces {
    use wayland_server::protocol::__interfaces::*;
    wayland_scanner::generate_interfaces!("protocols/dwl-ipc-unstable-v2.xml");
}

use self::__interfaces::*;
use wayland_server;
use wayland_server::protocol::*;

wayland_scanner::generate_server_code!("protocols/dwl-ipc-unstable-v2.xml");
