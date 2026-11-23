# Clicksmith

A native desktop auto clicker written in Rust. It supports Windows, macOS, and Linux/X11, with a small `egui` interface and Rust based input handling.

## Features

- Set the interval in hours, minutes, seconds, and milliseconds.
- Start clicking immediately, then repeat at the chosen interval.
- Add a random positive or negative interval offset.
- Choose left, right, or middle mouse button and single, double, or triple clicks.
- Repeat a fixed number of times or keep clicking until stopped.
- Click at the current pointer location or choose fixed screen coordinates.
- Pick a target location with a click anywhere on screen.
- Toggle clicking with a configurable global F6–F12 hotkey.
- Record mouse click positions and timing, then play the sequence back.
- Choose separate Black and Pink themes, other presets, or custom background and accent colors. Text contrast adjusts automatically, text fields stay white, and appearance and click settings persist between launches.
- Keep the main window on top of other windows.
- Check for GitHub releases from Settings, or opt into hourly checks with a prompt to install or open the release page.

The main window opens at a minimum of 720 × 760 points and can be resized larger or maximized to fit your display.

## Run

Install the stable Rust toolchain, then run:

```sh
cargo run --release
```

## Platform notes

- **Windows:** Global keyboard and mouse hooks can be affected by application permissions and privilege level. Run this app at the same privilege level as the target application.
- **macOS:** Grant Accessibility permission to the app or terminal used to launch it when macOS asks.
- **Linux:** Global hotkeys, picking, and recording use X11 events. The default mouse backend uses X11; Wayland support for global input capture is not provided by the listener, so those features require an X11 session or XWayland. Input simulation under Wayland may also be restricted by the compositor.

The clicker is designed for normal desktop automation. Some applications, elevated windows, games, and secure desktops may block simulated input.

## Build

```sh
cargo build --release
```

On Linux, the desktop and input crates may require your distribution's X11 development libraries. GitHub Actions checks builds on Windows, macOS, and Ubuntu.

Push a `v*` tag to create a GitHub release with platform builds attached. Tags containing a hyphen, such as `v0.1.1-alpha.1`, are published as pre-releases; stable tags such as `v0.1.0` are published as releases.

## Installers

GitHub releases include:

- **Windows:** NSIS `.exe` installer or `.msi` package.
- **macOS:** `.dmg` containing `Clicksmith.app`.
- **Linux:** `.deb`, `.rpm`, `.AppImage`, and `.flatpak` packages, plus a portable `.tar.gz`.

The Windows and macOS installers are unsigned, so their operating systems may show a security warning. Flatpak uses the Freedesktop 25.08 runtime and X11 access for global input controls; those controls still require an X11 or XWayland session.

Portable `.zip` and `.tar.gz` downloads support in-app update checks from Settings and can install a newer version, verify its asset digest when available, and restart. Installer and package-manager builds can check for updates and open the release page, but installation remains managed by the installer or package manager.

## License

MIT. See [LICENSE](LICENSE).
