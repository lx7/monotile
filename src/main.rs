// SPDX-License-Identifier: GPL-3.0-only

fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();

    let (mut event_loop, mut monotile) = monotile::Monotile::new();

    if std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some() {
        monotile::backend::winit::init(&mut event_loop, &mut monotile)?;
    } else {
        monotile::backend::drm::init(&mut event_loop, &mut monotile)?;
    }

    unsafe {
        std::env::remove_var("DISPLAY");
        std::env::set_var("WAYLAND_DISPLAY", &monotile.state.socket);
        std::env::set_var("XDG_SESSION_TYPE", "wayland");
        std::env::set_var("XDG_CURRENT_DESKTOP", "monotile");
    }

    monotile::spawn::autostart(monotile::config::AUTOSTART);

    event_loop.run(None, &mut monotile, |monotile| {
        monotile.state.flush_clients()
    })?;

    Ok(())
}

fn init_logging() {
    if let Ok(env_filter) = tracing_subscriber::EnvFilter::try_from_default_env() {
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
    } else {
        tracing_subscriber::fmt().init();
    }
}
