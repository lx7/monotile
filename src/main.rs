// SPDX-License-Identifier: GPL-3.0-only

use monotile::{
    Monotile, backend,
    config::{self, Args},
    spawn,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();

    let args = Args::parse();
    let config = config::load(args.config);
    let (mut event_loop, mut monotile) = Monotile::new(config);

    if std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some() {
        backend::winit::init(&mut event_loop, &mut monotile)?;
    } else {
        backend::drm::init(&mut event_loop, &mut monotile)?;
    }

    unsafe {
        std::env::remove_var("DISPLAY");
        std::env::set_var("WAYLAND_DISPLAY", &monotile.state.socket);
        std::env::set_var("XDG_SESSION_TYPE", "wayland");
        std::env::set_var("XDG_CURRENT_DESKTOP", "monotile");
    }

    spawn::autostart(args.autostart);

    event_loop.run(None, &mut monotile, |mt| mt.state.flush_clients())?;

    Ok(())
}

fn init_logging() {
    if let Ok(env_filter) = tracing_subscriber::EnvFilter::try_from_default_env() {
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
    } else {
        tracing_subscriber::fmt().init();
    }
}
