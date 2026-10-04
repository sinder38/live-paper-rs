<div align="center">
      <h1>Live Paper RS</h1>
    <h3>Play videos as your desktop background on Wayland</h3>

[![Rust](https://github.com/sinder38/live-paper-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/sinder38/live-paper-rs/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/live-paper.svg)](https://crates.io/crates/live-paper)
[![AUR](https://img.shields.io/aur/version/live-paper.svg)](https://aur.archlinux.org/packages/live-paper)
  <br>Support by giving your ⭐!

</div>

Render any mpv-playable video (or stream) onto the background layer of a Wayland compositor.

https://github.com/user-attachments/assets/d0034963-e6ca-4771-bda6-bbc3a4dcfbac

## Requirements

- A Wayland compositor supporting `wlr-layer-shell` (Sway, Hyprland, Niri, etc.)
- [`mpv`](https://mpv.io/) (libmpv), working EGL/OpenGL drivers (Mesa or vendor)

## Installation

### Arch Linux (AUR)
with an AUR helpers
```sh
paru -S live-paper
```
```sh
yay live-paper
```
or manually
```sh
git clone https://aur.archlinux.org/live-paper.git
cd live-paper && makepkg -si
```

### Ubuntu / Debian / Others

No prebuilt package yet; use the prebuilt GitHub release binary (no compilation needed):

```sh
sudo apt-get update
# Assuming your already have Wayland deps
sudo apt-get install -y libmpv2

curl -L https://github.com/sinder38/live-paper-rs/releases/latest/download/live-paper-linux-x86_64.tar.gz | tar xz
sudo install -Dm755 live-paper-linux-x86_64 /usr/local/bin/live-paper
```

### crates.io

Builds from source; the libmpv, EGL and Wayland development headers must be present.

```sh
cargo install live-paper
```

### GitHub Releases

Prebuilt dynamically-linked `x86_64` binary (still needs `mpv`, Mesa and Wayland
installed at runtime):

```sh
curl -L https://github.com/sinder38/live-paper-rs/releases/latest/download/live-paper-linux-x86_64.tar.gz | tar xz
install -Dm755 live-paper-linux-x86_64 ~/.local/bin/live-paper
```

## Quick start

Run `live-paper` from your compositor autostart (e.g. `exec-once = live-paper` in Hyprland).
It starts a daemon, so the wallpaper can be changed without restarting it.
`live-paper <command>` talks to the running daemon:

```sh
live-paper ~/Videos/wallpaper.mp4    # start the daemon with this video
live-paper                           # path from the config file, else a built-in test pattern

live-paper set ~/Videos/other.mp4    # swap the video
live-paper set --speed 1.5           # speed, mute and fill apply live too
live-paper pause                     # hold playback until `resume`
live-paper resume
live-paper toggle
live-paper query                     # what is playing, on which output, memory use
live-paper reload                    # re-read the config file
live-paper restart                   # replace the renderer process
live-paper kill                      # stop everything
```

### Without a daemon

```sh
live-paper --no-daemon ~/Videos/wallpaper.mp4
```

or `daemon = false` in the config. One process, no socket, no commands. It also
never replaces the leaking libmpv renderer, so memory grows over time.

### Configuration

Is optional, every field has a default. 
Config lives at
`$XDG_CONFIG_HOME/live-paper/config.toml` (usually `~/.config/live-paper/config.toml`).
Copy the sample to get started:

```sh
mkdir -p ~/.config/live-paper
# from the AUR/release install:
cp /usr/share/doc/live-paper/config.example.toml ~/.config/live-paper/config.toml
```

See [`config.example.toml`](config.example.toml) for all options (video path,
playback speed, mute, hardware decoding, mpv passthrough, layer settings).
Pass a specific file with `-c/--config-path`.

## Additional Acknowledgments

- https://github.com/GhostNaN/mpvpaper — inspiration and original mpvpaper project
- https://codeberg.org/LGFae/awww — a more mature project for static wallpapers with transitions

## TODO

1. A wallpaper per monitor
2. Publish on more package managers
3. Flag ignored config options

#### License

<sup>
Licensed under the <a href="LICENSE">MIT license</a>.
</sup>

<br>

<sub>
Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in live-paper-rs by you, as defined in the MIT, shall be 
licensed as above, without any additional terms or conditions.
</sub>
</content>
