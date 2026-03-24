use std::process::Command;

fn main() {
    let version = if let Ok(v) = std::env::var("MONOTILE_VERSION") {
        v
    } else if let Ok(out) = Command::new("git")
        .args(["describe", "--tags", "--always", "--dirty"])
        .output()
        && out.status.success()
        && let Ok(s) = String::from_utf8(out.stdout)
    {
        s.trim().trim_start_matches('v').to_string()
    } else {
        env!("CARGO_PKG_VERSION").to_string()
    };

    println!("cargo:rustc-env=MONOTILE_VERSION={version}");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");
}
