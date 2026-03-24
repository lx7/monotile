// SPDX-License-Identifier: GPL-3.0-only

use monotile::{
    Monotile, backend,
    config::{Args, Config},
    spawn,
};
use tracing::info;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();

    info!("monotile {}", env!("MONOTILE_VERSION"));

    let args = Args::parse();
    let config = Config::load(args.config).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });

    let (mut event_loop, mut monotile) = Monotile::new(config);

    if std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some() {
        info!("backend: winit");
        backend::winit::init(&mut event_loop, &mut monotile)?;
    } else {
        info!("backend: drm");
        backend::drm::init(&mut event_loop, &mut monotile)?;
    }

    unsafe {
        std::env::remove_var("DISPLAY");
        std::env::set_var("WAYLAND_DISPLAY", &monotile.state.socket);
        std::env::set_var("XDG_SESSION_TYPE", "wayland");
        std::env::set_var("XDG_CURRENT_DESKTOP", "monotile");
    }

    let autostart_pgid = spawn::autostart(args.autostart);

    event_loop.run(None, &mut monotile, |mt| mt.state.flush_clients())?;

    if let Some(pgid) = autostart_pgid {
        spawn::kill_autostart(pgid);
    }

    Ok(())
}

fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("monotile=info,warn"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}
