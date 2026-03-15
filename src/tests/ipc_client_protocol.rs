// SPDX-License-Identifier: GPL-3.0-only
// Client-side protocol bindings for integration tests.

#![allow(non_upper_case_globals, unused)]

pub mod monotile {
    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/monotile-ipc-unstable-v1.xml");
    }

    use self::__interfaces::*;
    use wayland_client;
    use wayland_client::protocol::*;

    wayland_scanner::generate_client_code!("protocols/monotile-ipc-unstable-v1.xml");
}

pub mod dwl {
    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/dwl-ipc-unstable-v2.xml");
    }

    use self::__interfaces::*;
    use wayland_client;
    use wayland_client::protocol::*;

    wayland_scanner::generate_client_code!("protocols/dwl-ipc-unstable-v2.xml");
}
