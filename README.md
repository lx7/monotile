<p align="left"><img src="assets/monotile.svg" alt="monotile" width="200"></p>

A small and light tiling Wayland compositor in Rust.

> [!NOTE]
> monotile is under active development. Usable as a daily driver, but some features are still missing (multi-monitor, screen sharing etc).

## Features

**Window management:**
- [x] Dynamic tiling layout (main/stack)
- [x] Floating windows (auto-detected from hints, rules, or toggled)
- [x] Tag system with per-tag layout and window order
- [x] Keyboard-driven (mouse optional)
- [x] Rule-based runtime configuration
- [x] Autostart (shell script)
- [x] Smart borders and smart gaps
- [x] Focus-follows-mouse
- [x] Screen lock and idle protocols

**Rendering and backend:**
- [x] DRM/KMS backend with session handling
- [x] Hardware accelerated (EGL/GBM render path, GLSL shaders)
- [x] Server-side decorations (rounded corners, shadows, borders)
- [x] Damage tracking and direct scanout
- [x] Layer shell (panels, bars, overlays)
- [x] Clipboard protocols (wl-copy/wl-paste)
- [x] IPC for status bars (monotile-ipc-v1, dwl-ipc-v2)

**Not yet implemented:**
- [ ] Multi-monitor support
- [ ] Screen sharing
- [ ] Output management
- [ ] Gamma control
- [ ] HiDPI / multi-DPI support
- [ ] Hide cursor when typing
- [ ] Monocle layout

## Building monotile

Requires Rust 1.85 or later.

### NixOS

Add the flake to your system config and rebuild, or install to your user profile:

```bash
nix profile add github:lx7/monotile
```

For development:

```bash
nix develop
cargo build --release
```


### Arch Linux

```bash
sudo pacman -S rust pkgconf wayland libxkbcommon libdrm libinput seatd mesa libdisplay-info
cargo build --release
```


### Debian / Ubuntu

```bash
# rust toolchain (1.85+)
# Debian 13+ / Ubuntu 25.04+:
sudo apt-get install -y cargo
# older distros:
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# build dependencies
sudo apt-get install -y libwayland-dev libxkbcommon-dev libgbm-dev libdrm-dev libudev-dev libinput-dev libseat-dev libegl1-mesa-dev libdisplay-info-dev

# runtime dependencies
sudo apt-get install -y libwayland-server0 libxkbcommon0 libgbm1 libudev1 libinput10 libseat1 libegl1 libdisplay-info2

cargo build --release
```


## Configuration

Configuration files are in `$XDG_CONFIG_HOME/monotile/`:
- `config.ron`: Rule-based configuration
- `autostart.sh`: Commands to run on startup

On first run, monotile creates a default config if none exists. Edit it to customize keybindings, layout, appearance, and input devices.

Custom paths can be specified: `monotile [-c <config>] [-s <autostart>]`

monotile is not a desktop environment. You need a terminal emulator and an application launcher. The defaults use [foot](https://codeberg.org/dnkl/foot) and [fuzzel](https://codeberg.org/dnkl/fuzzel) - adjust the config to match your setup.


## Essential keybindings

| Key | Action |
|-----|--------|
| <kbd>Super</kbd>+<kbd>Return</kbd> | Open terminal (foot) |
| <kbd>Super</kbd>+<kbd>D</kbd> | Open launcher (fuzzel) |
| <kbd>Super</kbd>+<kbd>Shift</kbd>+<kbd>Q</kbd> | Close window |
| <kbd>Super</kbd>+<kbd>Space</kbd> | Toggle fullscreen |
| <kbd>Super</kbd>+<kbd>Shift</kbd>+<kbd>Space</kbd> | Toggle floating |
| <kbd>Super</kbd>+<kbd>←</kbd> | Focus previous window |
| <kbd>Super</kbd>+<kbd>→</kbd> | Focus next window |
| <kbd>Super</kbd>+<kbd>Shift</kbd>+<kbd>←</kbd> | Swap window with previous in stack |
| <kbd>Super</kbd>+<kbd>Shift</kbd>+<kbd>→</kbd> | Swap window with next in stack |
| <kbd>Super</kbd>+<kbd>1</kbd>..<kbd>9</kbd> | Switch to tag |
| <kbd>Super</kbd>+<kbd>Shift</kbd>+<kbd>1</kbd>..<kbd>9</kbd> | Move window to tag |
| <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>Backspace</kbd> | Quit |
| <kbd>Super</kbd>+<kbd>LMB</kbd> | Drag to move floating windows |
| <kbd>Super</kbd>+<kbd>RMB</kbd> | Drag to resize floating windows |


## Worth checking out
- [Cosmic](https://github.com/pop-os/cosmic-epoch): Wayland desktop with tiling compositor by System76.
- [niri](https://github.com/YaLTeR/niri): Continuous side-scrolling-tiling wayland compositor.
- [dwl](https://codeberg.org/dwl/dwl): dwm for wayland.
- [bspwm](https://github.com/baskerville/bspwm): bsp-tree based tiling window manager for X11.


## Acknowledgements

- [smithay](https://github.com/Smithay/smithay) - monotile is based on the Smithay Wayland library, and the smallvil example.
- [niri](https://github.com/YaLTeR/niri) - Clipping and decoration shader technique. Also, the idea for the integration test fixture.


