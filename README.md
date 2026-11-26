# Clicksmith

A native desktop auto clicker written in Rust, with a separate native Android app. The desktop app supports Windows, macOS, and Linux/X11, with a small `egui` interface and Rust based input handling.

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
- Choose a stable or beta update channel on desktop. Beta includes stable releases; Android updates remain managed by Google Play.

The main window opens at a minimum of 720 × 760 points and can be resized larger or maximized to fit your display.

## Android

The Android app is in [`android/`](android/) and can be opened in Android Studio. It provides saved tap intervals, optional random timing variation, single/double/triple taps, finite or continuous repeats, a movable tap target, and floating Start/Stop controls.

Android requires you to accept the in-app accessibility disclosure, then enable **Clicksmith tap controls** in Accessibility settings. This system permission is used to place the target and controls over other apps and dispatch taps when you press Start. The service does not inspect other apps' screen contents or send accessibility data to Clicksmith servers. Tap settings and target position are saved in app preferences and may be included in Android backups if device backup is enabled. The overlay stays visible while Clicksmith is in the background, closes when you finish or dismiss the app task, and returns when you reopen the app. Drag the target onto the desired location; Stop is available from the floating controls at all times.

See the [Clicksmith Android Privacy Policy](https://scorpionlegend.github.io/clicksmith/privacy-policy/).

Build a debug APK from the repository root.

```powershell
cd android
.\gradlew.bat :app:assembleDebug
```

On macOS/Linux, run `cd android && bash gradlew :app:assembleDebug`. The debug APK is written to `android/app/build/outputs/apk/debug/`.

Android beta GitHub releases include an installable APK. To publish a signed release APK instead, configure the repository Actions secret `ANDROID_KEYSTORE_BASE64` with your release keystore encoded as Base64, and `ANDROID_KEYSTORE_PASSWORD` with its store/key password. Set the repository Actions variable `ANDROID_SIGNING_ENABLED` to `true` to enable signing. The release key alias is `clicksmith`. Keep an offline backup of the keystore and never commit it; losing the key prevents updates to installations signed with it.

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

Push a stable `v*` tag matching the Cargo package version to publish the desktop builds as a public GitHub release. Push a hyphenated beta tag matching the Android `versionName` to publish an Android-only prerelease APK; beta tags do not build or publish desktop binaries.

## Installers

GitHub releases include:

- **Windows:** NSIS `.exe` installer or `.msi` package.
- **macOS:** `.dmg` containing `Clicksmith.app`.
- **Linux:** `.deb`, `.rpm`, `.AppImage`, and `.flatpak` packages, plus a portable `.tar.gz`.

The Windows and macOS installers are unsigned, so their operating systems may show a security warning. Flatpak uses the Freedesktop 25.08 runtime and X11 access for global input controls; those controls still require an X11 or XWayland session.

Portable `.zip` and `.tar.gz` downloads support in-app updates from Settings, with a choice of stable-only or beta (beta also receives stable releases). The Windows setup installer downloads and runs its SHA-256-verified setup package, then restarts Clicksmith. MSI, macOS, and Linux package-manager installations can check for updates and open the release page, but installation remains managed by their installer or package manager. Android beta access is managed through Google Play testing tracks.

## License

MIT. See [LICENSE](LICENSE).
