// SPDX-License-Identifier: GPL-3.0-only

use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::config;

pub fn autostart(explicit: Option<PathBuf>) -> Option<i32> {
    let path = config::resolve_autostart(explicit);
    if !path.exists() {
        return None;
    }

    let mut proc = Command::new("sh");
    proc.arg(path.to_string_lossy().into_owned())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .process_group(0);

    match proc.spawn() {
        Ok(child) => {
            let pgid = child.id() as i32;
            tracing::info!("autostart pgid={pgid}");
            std::mem::forget(child);
            Some(pgid)
        }
        Err(e) => {
            tracing::error!("failed to run autostart: {e}");
            None
        }
    }
}

pub fn kill_autostart(pgid: i32) {
    tracing::info!("killing autostart group pgid={pgid}");
    let _ = Command::new("kill")
        .args(["-TERM", "--", &format!("-{pgid}")])
        .spawn();
}

pub fn spawn(cmd: &str, args: &[String], log: bool) {
    let mut proc = Command::new(cmd);
    proc.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(if log { Stdio::inherit() } else { Stdio::null() });
    match proc.spawn() {
        Ok(mut child) => {
            tracing::debug!("{cmd} {}", args.join(" "));
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => tracing::error!("failed to start {cmd}: {e}"),
    }
}

pub fn spawn_shell(command: &str) {
    spawn("sh", &["-c".into(), command.into()], false);
}

pub fn notify(level: &str, title: &str, msg: &str) {
    spawn(
        "notify-send",
        &[
            "-a".into(),
            "monotile".into(),
            "-u".into(),
            level.into(),
            title.into(),
            msg.into(),
        ],
        false,
    );
}
