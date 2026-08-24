<div align="center">
      <h1>Live Paper RS</h1>
    <h3>Play videos as your desktop background on Wayland</h3>

[![Rust](https://github.com/sinder38/live-paper-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/sinder38/live-paper-rs/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/live-paper.svg)](https://crates.io/crates/live-paper)
[![AUR](https://img.shields.io/aur/version/live-paper.svg)](https://aur.archlinux.org/packages/live-paper)
  <br>Support by giving your ⭐!

</div>


<!-- TODO: showcase gif/screenshot once a properly-licensed wallpaper is available -->

Render any mpv-playable video (or stream) onto the background layer of a Wayland compositor.

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

```sh
# Play a video file as your wallpaper
live-paper ~/Videos/wallpaper.mp4

# Any mpv-compatible source works, including streams
live-paper "https://example.com/clip.mp4"

# No argument -> falls back to the config file, then a built-in test pattern
live-paper
```

Run it from your compositor's autostart (e.g. `exec-once = live-paper ~/Videos/wallpaper.mp4` in Hyprland).

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

1. Daemon persistence and dynamic wallpaper swap
3. Publish on more package managers
4. Flag ignored config options

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
