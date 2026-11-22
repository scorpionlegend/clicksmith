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
- Check for GitHub releases at startup, install a newer version automatically, then restart.

The main window has a fixed size so its controls remain visible together.

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

Push a `v*` tag to create a GitHub pre-release with platform builds attached. Each tagged release is marked as a pre-release.

Release builds check the public GitHub release list when they start. If a newer version is available, Clicksmith downloads the matching platform archive, verifies its GitHub-published SHA-256 digest when provided, replaces its executable, and restarts. Debug builds skip update checks. The update check requires internet access and write access to the installed executable's folder; if an update cannot be applied, Clicksmith starts normally and shows the reason in its status line.

## License

MIT. See [LICENSE](LICENSE).
