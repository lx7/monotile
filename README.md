# monotile

A minimalist tiling Wayland compositor in Rust.

> [!IMPORTANT]
> monotile is under active development and not yet ready for daily use.
> There is no DRM backend yet. monotile currently runs nested inside another
> Wayland compositor using the winit backend.

## Build

### NixOS

```bash
nix develop
cargo build
```

### Debian / Ubuntu

```bash
# rust toolchain (1.85+)
# Debian 13+ / Ubuntu 25.04+:
sudo apt-get install -y cargo
# older distros:
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# build dependencies
sudo apt-get install -y libwayland-dev libxkbcommon-dev libgbm-dev libudev-dev libinput-dev libseat-dev libegl1-mesa-dev

# runtime dependencies
sudo apt-get install -y libwayland-server0 libxkbcommon0 libgbm1 libudev1 libinput10 libseat1 libegl1

cargo build
```

