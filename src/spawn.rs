// SPDX-License-Identifier: GPL-3.0-only

use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::config;

pub fn autostart(explicit: Option<PathBuf>) {
    let path = config::resolve_autostart(explicit);
    if path.exists() {
        spawn("sh", &[path.to_string_lossy().into_owned()], true);
    }
}

pub fn spawn_shell(command: &str) {
    spawn("sh", &["-c".into(), command.into()], false);
}

pub fn spawn(cmd: &str, args: &[String], log: bool) {
    let mut proc = Command::new(cmd);
    proc.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(if log { Stdio::inherit() } else { Stdio::null() });
    match proc.spawn() {
        Ok(mut child) => {
            tracing::info!("{cmd} {}", args.join(" "));
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(err) => tracing::error!("failed to start {cmd}: {err}"),
    }
}
