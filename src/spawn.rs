// SPDX-License-Identifier: GPL-3.0-only

use std::process::{Command, Stdio};

pub fn autostart(cmds: &[(&str, &[&str])]) {
    for (cmd, args) in cmds {
        spawn(cmd, args);
    }
}

pub fn spawn(cmd: &str, args: &[&str]) {
    let mut proc = Command::new(cmd);
    proc.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    match proc.spawn() {
        Ok(mut child) => {
            tracing::info!("spawned {cmd}");
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(err) => tracing::error!("failed to spawn {cmd}: {err}"),
    }
}
