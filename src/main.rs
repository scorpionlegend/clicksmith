#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

use eframe::egui;
use enigo::{Button as EnigoButton, Coordinate, Direction, Enigo, Mouse, Settings};
use rdev::{Button as GlobalButton, Event, EventType, Key};

#[cfg(all(target_os = "windows", feature = "windows-nsis"))]
use std::io::Read;

#[cfg(all(target_os = "windows", feature = "windows-nsis"))]
use sha2::{Digest, Sha256};

#[cfg(all(target_os = "windows", feature = "windows-nsis"))]
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
    process::Command,
    time::UNIX_EPOCH,
};

const CLICK_GAP: Duration = Duration::from_millis(55);
const UI_REFRESH: Duration = Duration::from_millis(16);
const HOTKEY_DEBOUNCE: Duration = Duration::from_millis(300);
const AUTO_UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);
#[cfg(all(target_os = "windows", feature = "windows-nsis"))]
const WINDOWS_SETUP_ASSET: &str = "clicksmith-windows-x86_64-setup.exe";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let icon_image =
        image::load_from_memory(include_bytes!("../packaging/linux/clicksmith.png"))?.into_rgba8();
    let (icon_width, icon_height) = icon_image.dimensions();
    let icon = egui::IconData {
        rgba: icon_image.into_raw(),
        width: icon_width,
        height: icon_height,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([420.0, 500.0])
            .with_min_inner_size([420.0, 500.0])
            .with_resizable(false)
            .with_maximize_button(false)
            .with_icon(Arc::new(icon)),
        window_builder: Some(Box::new(|viewport| {
            viewport.with_inner_size([420.0, 500.0])
        })),
        ..Default::default()
    };

    eframe::run_native(
        "Clicksmith",
        options,
        Box::new(move |creation_context| Ok(Box::new(ClickerApp::new(creation_context.storage)))),
    )?;
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum UpdateCheckResult {
    UpdateAvailable(String),
    UpToDate,
    DevelopmentBuild,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UpdateInstallResult {
    Updated,
    UpToDate,
    #[cfg(all(target_os = "windows", feature = "windows-nsis"))]
    InstallerStarted,
}

fn configured_updater(
    release_tag: Option<String>,
) -> Result<self_update::backends::github::Update, String> {
    let mut updater = self_update::backends::github::Update::configure();
    updater
        .repo_owner("scorpionlegend")
        .repo_name("clicksmith")
        .bin_name("clicksmith")
        .asset_identifier(release_asset_identifier())
        .current_version(self_update::cargo_crate_version!())
        .update_strategy(self_update::UpdateStrategy::Latest)
        .timeout(Duration::from_secs(30))
        .check_install_path_writable(true)
        .unattended();
    if let Some(release_tag) = release_tag {
        updater.release_tag(release_tag);
    }
    updater.build().map_err(|error| error.to_string())
}

fn latest_eligible_version(
    versions: impl IntoIterator<Item = String>,
    channel: UpdateChannel,
) -> Result<Option<String>, String> {
    let mut latest: Option<(semver::Version, String)> = None;
    for version_text in versions {
        let version = semver::Version::parse(&version_text).map_err(|error| error.to_string())?;
        if !channel.allows(&version) {
            continue;
        }

        if latest.as_ref().map_or(true, |(latest_version, _)| {
            version.cmp(latest_version).is_gt()
        }) {
            latest = Some((version, version_text));
        }
    }
    Ok(latest.map(|(_, version)| version))
}

fn release_supports_current_platform(release: &self_update::Release) -> bool {
    #[cfg(all(target_os = "windows", feature = "windows-nsis"))]
    {
        release
            .assets()
            .iter()
            .any(|asset| asset.name() == WINDOWS_SETUP_ASSET)
    }
    #[cfg(not(all(target_os = "windows", feature = "windows-nsis")))]
    {
        release
            .assets()
            .iter()
            .any(|asset| asset.name().contains(release_asset_identifier()))
    }
}

fn check_for_updates(channel: UpdateChannel) -> Result<UpdateCheckResult, String> {
    if cfg!(debug_assertions) {
        return Ok(UpdateCheckResult::DevelopmentBuild);
    }

    let releases = configured_updater(None)?
        .get_newer_releases()
        .map_err(|error| error.to_string())?;
    let version = latest_eligible_version(
        releases
            .into_iter()
            .filter(release_supports_current_platform)
            .map(|release| release.version().to_owned()),
        channel,
    )?;
    match version {
        Some(version) => Ok(UpdateCheckResult::UpdateAvailable(version)),
        None => Ok(UpdateCheckResult::UpToDate),
    }
}

fn update_checks_supported() -> bool {
    !cfg!(debug_assertions)
}

fn in_app_update_supported() -> bool {
    !cfg!(debug_assertions)
        && (!cfg!(feature = "package-managed")
            || cfg!(all(target_os = "windows", feature = "windows-nsis")))
}

fn release_page_url(version: &str) -> String {
    format!("https://github.com/scorpionlegend/clicksmith/releases/tag/v{version}")
}

fn install_update(version: &str, channel: UpdateChannel) -> Result<UpdateInstallResult, String> {
    if cfg!(debug_assertions) {
        return Err("Update installation is disabled in debug builds".to_owned());
    }
    let parsed_version = semver::Version::parse(version).map_err(|error| error.to_string())?;
    if !channel.allows(&parsed_version) {
        return Err(format!(
            "Version {version} is not available in the {} channel",
            channel.label()
        ));
    }
    if cfg!(feature = "package-managed") {
        return install_package_managed_update(version);
    }

    configured_updater(Some(format!("v{version}")))
        .and_then(|updater| updater.update().map_err(|error| error.to_string()))
        .map(|status| {
            if status.is_updated() {
                UpdateInstallResult::Updated
            } else {
                UpdateInstallResult::UpToDate
            }
        })
}

fn install_package_managed_update(version: &str) -> Result<UpdateInstallResult, String> {
    #[cfg(all(target_os = "windows", feature = "windows-nsis"))]
    {
        install_windows_setup_update(version)
    }
    #[cfg(not(all(target_os = "windows", feature = "windows-nsis")))]
    {
        let _ = version;
        Err("Updates for this installation are managed externally".to_owned())
    }
}

#[cfg(all(target_os = "windows", feature = "windows-nsis"))]
fn windows_setup_asset(
    release: &self_update::Release,
) -> Result<&self_update::ReleaseAsset, String> {
    release
        .assets()
        .iter()
        .find(|asset| asset.name() == WINDOWS_SETUP_ASSET)
        .ok_or_else(|| format!("Release does not contain {WINDOWS_SETUP_ASSET}"))
}

#[cfg(all(target_os = "windows", feature = "windows-nsis"))]
fn verify_sha256_digest(reader: &mut impl Read, digest: Option<&str>) -> Result<(), String> {
    let digest = digest.ok_or_else(|| {
        "GitHub did not provide an installer checksum; refusing to run it.".to_owned()
    })?;
    let (algorithm, expected) = digest
        .split_once(':')
        .ok_or_else(|| "GitHub returned an invalid installer checksum.".to_owned())?;
    if algorithm != "sha256"
        || expected.len() != 64
        || !expected.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("GitHub returned an unsupported installer checksum.".to_owned());
    }

    let mut hasher = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err("Downloaded installer failed its SHA-256 checksum.".to_owned())
    }
}

#[cfg(all(target_os = "windows", feature = "windows-nsis"))]
fn install_windows_setup_update(version: &str) -> Result<UpdateInstallResult, String> {
    let release = configured_updater(None)?
        .get_release_version(&format!("v{version}"))
        .map_err(|error| error.to_string())?;
    let asset = windows_setup_asset(&release)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let temp_dir = std::env::temp_dir();
    let stem = format!("clicksmith-update-{}-{nonce}", std::process::id());
    let installer_path = temp_dir.join(format!("{stem}.exe"));
    let script_path = temp_dir.join(format!("{stem}.ps1"));

    let mut installer_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&installer_path)
        .map_err(|error| error.to_string())?;
    let download_result = {
        let mut download = self_update::Download::from_url(asset.download_url());
        download
            .request_header("Accept", "application/octet-stream")
            .timeout(Duration::from_secs(300))
            .max_download_size(128 * 1024 * 1024)
            .download_to(&mut installer_file)
            .map_err(|error| error.to_string())
    };
    drop(installer_file);
    if let Err(error) = download_result {
        let _ = fs::remove_file(&installer_path);
        return Err(error);
    }

    let mut downloaded_installer = File::open(&installer_path).map_err(|error| {
        let _ = fs::remove_file(&installer_path);
        error.to_string()
    })?;
    if let Err(error) = verify_sha256_digest(&mut downloaded_installer, asset.digest()) {
        drop(downloaded_installer);
        let _ = fs::remove_file(&installer_path);
        return Err(error);
    }
    drop(downloaded_installer);

    let app_path = std::env::current_exe().map_err(|error| {
        let _ = fs::remove_file(&installer_path);
        error.to_string()
    })?;
    let installer_path_text = installer_path.to_str().ok_or_else(|| {
        let _ = fs::remove_file(&installer_path);
        "Downloaded installer path is not valid Unicode.".to_owned()
    })?;
    let app_path_text = app_path.to_str().ok_or_else(|| {
        let _ = fs::remove_file(&installer_path);
        "Clicksmith executable path is not valid Unicode.".to_owned()
    })?;
    let script = format!(
        "$ErrorActionPreference = 'Stop'\n\
         $parentProcessId = {}\n\
         $installerPath = {}\n\
         $appPath = {}\n\
         $scriptPath = $MyInvocation.MyCommand.Path\n\
         try {{\n\
             Wait-Process -Id $parentProcessId -ErrorAction SilentlyContinue\n\
             Start-Process -FilePath $installerPath -Wait\n\
         }} finally {{\n\
             try {{\n\
                 Start-Process -FilePath $appPath\n\
             }} finally {{\n\
                 Remove-Item -LiteralPath $installerPath -Force -ErrorAction SilentlyContinue\n\
                 Remove-Item -LiteralPath $scriptPath -Force -ErrorAction SilentlyContinue\n\
             }}\n\
         }}\n",
        std::process::id(),
        powershell_literal(installer_path_text),
        powershell_literal(app_path_text),
    );
    write_powershell_script(&script_path, &script).map_err(|error| {
        let _ = fs::remove_file(&installer_path);
        error.to_string()
    })?;

    launch_update_script(&script_path).map_err(|error| {
        let _ = fs::remove_file(&installer_path);
        let _ = fs::remove_file(&script_path);
        error.to_string()
    })?;
    Ok(UpdateInstallResult::InstallerStarted)
}

#[cfg(all(target_os = "windows", feature = "windows-nsis"))]
fn powershell_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(all(target_os = "windows", feature = "windows-nsis"))]
fn write_powershell_script(path: &Path, script: &str) -> std::io::Result<()> {
    let mut encoded = vec![0xFF, 0xFE];
    for unit in script.encode_utf16() {
        encoded.extend_from_slice(&unit.to_le_bytes());
    }
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    if let Err(error) = file.write_all(&encoded) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(error);
    }
    Ok(())
}

#[cfg(all(target_os = "windows", feature = "windows-nsis"))]
fn launch_update_script(path: &Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;

    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-File",
        ])
        .arg(path)
        .creation_flags(0x08000000)
        .spawn()
        .map(|_| ())
}

#[cfg(target_os = "windows")]
fn release_asset_identifier() -> &'static str {
    "windows-x86_64"
}

#[cfg(target_os = "linux")]
fn release_asset_identifier() -> &'static str {
    "linux-x86_64"
}

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
fn release_asset_identifier() -> &'static str {
    "macos-x86_64"
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn release_asset_identifier() -> &'static str {
    "macos-aarch64"
}

#[cfg(all(
    target_os = "macos",
    not(any(target_arch = "x86_64", target_arch = "aarch64"))
))]
fn release_asset_identifier() -> &'static str {
    "macos"
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn release_asset_identifier() -> &'static str {
    "clicksmith"
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum UpdateChannel {
    #[default]
    Stable,
    Beta,
}

impl UpdateChannel {
    fn label(self) -> &'static str {
        match self {
            Self::Stable => "Stable",
            Self::Beta => "Beta",
        }
    }

    fn allows(self, version: &semver::Version) -> bool {
        self == Self::Beta || version.pre.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum ClickButton {
    Left,
    Right,
    Middle,
}

impl ClickButton {
    fn label(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Middle => "Middle",
        }
    }

    fn enigo(self) -> EnigoButton {
        match self {
            Self::Left => EnigoButton::Left,
            Self::Right => EnigoButton::Right,
            Self::Middle => EnigoButton::Middle,
        }
    }

    fn from_global(button: GlobalButton) -> Option<Self> {
        match button {
            GlobalButton::Left => Some(Self::Left),
            GlobalButton::Right => Some(Self::Right),
            GlobalButton::Middle => Some(Self::Middle),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum ClickType {
    Single,
    Double,
    Triple,
}

impl ClickType {
    fn label(self) -> &'static str {
        match self {
            Self::Single => "Single",
            Self::Double => "Double",
            Self::Triple => "Triple",
        }
    }

    fn count(self) -> u8 {
        match self {
            Self::Single => 1,
            Self::Double => 2,
            Self::Triple => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum RepeatMode {
    UntilStopped,
    Count,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum CursorMode {
    Current,
    Fixed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum Hotkey {
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
}

impl Hotkey {
    fn label(self) -> &'static str {
        match self {
            Self::F6 => "F6",
            Self::F7 => "F7",
            Self::F8 => "F8",
            Self::F9 => "F9",
            Self::F10 => "F10",
            Self::F11 => "F11",
            Self::F12 => "F12",
        }
    }

    fn key(self) -> Key {
        match self {
            Self::F6 => Key::F6,
            Self::F7 => Key::F7,
            Self::F8 => Key::F8,
            Self::F9 => Key::F9,
            Self::F10 => Key::F10,
            Self::F11 => Key::F11,
            Self::F12 => Key::F12,
        }
    }
}

#[derive(Clone, Copy)]
struct ClickSettings {
    interval: Duration,
    random_offset_ms: u64,
    button: ClickButton,
    click_type: ClickType,
    repeat_mode: RepeatMode,
    repeat_count: u64,
    cursor_mode: CursorMode,
    x: i32,
    y: i32,
}

#[derive(Clone, Debug)]
struct RecordedClick {
    at: Duration,
    x: i32,
    y: i32,
    button: ClickButton,
}

enum RunPlan {
    Clicker(ClickSettings),
    Playback(Vec<RecordedClick>),
}

struct Worker {
    stop: Sender<()>,
    thread: thread::JoinHandle<()>,
    clicks: Arc<AtomicU64>,
    result: Receiver<Option<String>>,
}

impl Worker {
    fn start(plan: RunPlan) -> Result<Self, String> {
        let (stop_tx, stop_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        let clicks = Arc::new(AtomicU64::new(0));
        let thread_clicks = Arc::clone(&clicks);

        let worker_thread = thread::Builder::new()
            .name("clicksmith".to_owned())
            .spawn(move || {
                let result = run_plan(plan, &stop_rx, &thread_clicks);
                let _ = result_tx.send(result.err());
            })
            .map_err(|error| format!("Could not start the click worker: {error}"))?;

        Ok(Self {
            stop: stop_tx,
            thread: worker_thread,
            clicks,
            result: result_rx,
        })
    }

    fn click_count(&self) -> u64 {
        self.clicks.load(Ordering::Relaxed)
    }

    fn request_stop(&self) {
        let _ = self.stop.send(());
    }
}

fn run_plan(plan: RunPlan, stop: &Receiver<()>, clicks: &AtomicU64) -> Result<(), String> {
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|error| format!("Could not initialize mouse input: {error}"))?;

    match plan {
        RunPlan::Clicker(settings) => run_clicker(&mut enigo, settings, stop, clicks),
        RunPlan::Playback(sequence) => run_playback(&mut enigo, &sequence, stop, clicks),
    }
}

fn run_clicker(
    enigo: &mut Enigo,
    settings: ClickSettings,
    stop: &Receiver<()>,
    clicks: &AtomicU64,
) -> Result<(), String> {
    let mut random_state = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
        | 1;
    let iterations = match settings.repeat_mode {
        RepeatMode::UntilStopped => None,
        RepeatMode::Count => Some(settings.repeat_count.max(1)),
    };
    let mut completed = 0_u64;
    let mut first_iteration = true;

    loop {
        if first_iteration {
            first_iteration = false;
        } else {
            let delay = randomized_interval(
                settings.interval,
                settings.random_offset_ms,
                &mut random_state,
            );
            if wait_for_stop(stop, delay) {
                return Ok(());
            }
        }

        if let CursorMode::Fixed = settings.cursor_mode {
            enigo
                .move_mouse(settings.x, settings.y, Coordinate::Abs)
                .map_err(|error| format!("Could not move the cursor: {error}"))?;
        }

        for click_index in 0..settings.click_type.count() {
            enigo
                .button(settings.button.enigo(), Direction::Click)
                .map_err(|error| format!("Mouse click failed: {error}"))?;
            clicks.fetch_add(1, Ordering::Relaxed);

            if click_index + 1 < settings.click_type.count() && wait_for_stop(stop, CLICK_GAP) {
                return Ok(());
            }
        }

        completed += 1;
        if iterations.is_some_and(|limit| completed >= limit) {
            return Ok(());
        }
    }
}

fn run_playback(
    enigo: &mut Enigo,
    sequence: &[RecordedClick],
    stop: &Receiver<()>,
    clicks: &AtomicU64,
) -> Result<(), String> {
    let playback_started = Instant::now();

    for action in sequence {
        let remaining = action.at.saturating_sub(playback_started.elapsed());
        if wait_for_stop(stop, remaining) {
            return Ok(());
        }

        enigo
            .move_mouse(action.x, action.y, Coordinate::Abs)
            .map_err(|error| format!("Could not move the cursor during playback: {error}"))?;
        enigo
            .button(action.button.enigo(), Direction::Click)
            .map_err(|error| format!("Playback click failed: {error}"))?;
        clicks.fetch_add(1, Ordering::Relaxed);
    }

    Ok(())
}

fn wait_for_stop(stop: &Receiver<()>, duration: Duration) -> bool {
    match stop.recv_timeout(duration) {
        Ok(()) | Err(RecvTimeoutError::Disconnected) => true,
        Err(RecvTimeoutError::Timeout) => false,
    }
}

fn randomized_interval(base: Duration, offset_ms: u64, state: &mut u64) -> Duration {
    if offset_ms == 0 {
        return base.max(Duration::from_millis(1));
    }

    let mut value = *state;
    value ^= value << 13;
    value ^= value >> 7;
    value ^= value << 17;
    *state = value;

    let spread = offset_ms.min(i64::MAX as u64 / 2);
    let width = spread.saturating_mul(2).saturating_add(1);
    let offset = (value % width) as i64 - spread as i64;
    let base_ms = base.as_millis().min(i64::MAX as u128) as i64;
    let adjusted_ms = (base_ms + offset).max(1) as u64;
    Duration::from_millis(adjusted_ms)
}

enum ListenerMessage {
    Event(Event),
    Error(String),
}

const SETTINGS_STORAGE_KEY: &str = "clicksmith_settings";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum ThemeChoice {
    #[default]
    ClassicGray,
    Black,
    Pink,
    BabyBlue,
    Lavender,
    Mint,
    WarmLight,
    Custom,
}

impl ThemeChoice {
    const ALL: [Self; 8] = [
        Self::ClassicGray,
        Self::Black,
        Self::Pink,
        Self::BabyBlue,
        Self::Lavender,
        Self::Mint,
        Self::WarmLight,
        Self::Custom,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::ClassicGray => "Classic gray",
            Self::Black => "Black",
            Self::Pink => "Pink",
            Self::BabyBlue => "Baby blue",
            Self::Lavender => "Lavender",
            Self::Mint => "Mint",
            Self::WarmLight => "Warm light",
            Self::Custom => "Custom colors",
        }
    }
}

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct AppSettings {
    theme: ThemeChoice,
    custom_background: [u8; 3],
    custom_accent: [u8; 3],
    always_on_top: bool,
    auto_update_enabled: bool,
    update_channel: UpdateChannel,
    hours: u32,
    minutes: u32,
    seconds: u32,
    milliseconds: u32,
    random_offset_enabled: bool,
    random_offset_ms: u32,
    button: ClickButton,
    click_type: ClickType,
    repeat_mode: RepeatMode,
    repeat_count: u64,
    cursor_mode: CursorMode,
    cursor_x: i32,
    cursor_y: i32,
    hotkey: Hotkey,
    hotkey_enabled: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::ClassicGray,
            custom_background: [228, 231, 235],
            custom_accent: [74, 134, 202],
            always_on_top: false,
            auto_update_enabled: false,
            update_channel: UpdateChannel::Stable,
            hours: 0,
            minutes: 0,
            seconds: 0,
            milliseconds: 100,
            random_offset_enabled: false,
            random_offset_ms: 40,
            button: ClickButton::Left,
            click_type: ClickType::Single,
            repeat_mode: RepeatMode::UntilStopped,
            repeat_count: 1,
            cursor_mode: CursorMode::Current,
            cursor_x: 900,
            cursor_y: 966,
            hotkey: Hotkey::F6,
            hotkey_enabled: true,
        }
    }
}

struct ThemePalette {
    background: egui::Color32,
    window: egui::Color32,
    accent: egui::Color32,
}

impl AppSettings {
    fn palette(&self) -> ThemePalette {
        use egui::Color32;

        let rgb = |red, green, blue| Color32::from_rgb(red, green, blue);
        match self.theme {
            ThemeChoice::ClassicGray => ThemePalette {
                background: rgb(235, 237, 240),
                window: rgb(247, 248, 250),
                accent: rgb(47, 126, 199),
            },
            ThemeChoice::Black => ThemePalette {
                background: rgb(20, 18, 24),
                window: rgb(34, 29, 39),
                accent: rgb(72, 130, 206),
            },
            ThemeChoice::Pink => ThemePalette {
                background: rgb(247, 214, 230),
                window: rgb(255, 237, 246),
                accent: rgb(218, 55, 133),
            },
            ThemeChoice::BabyBlue => ThemePalette {
                background: rgb(211, 234, 248),
                window: rgb(231, 245, 253),
                accent: rgb(55, 139, 194),
            },
            ThemeChoice::Lavender => ThemePalette {
                background: rgb(37, 32, 49),
                window: rgb(52, 44, 68),
                accent: rgb(174, 142, 255),
            },
            ThemeChoice::Mint => ThemePalette {
                background: rgb(27, 42, 36),
                window: rgb(40, 60, 51),
                accent: rgb(104, 219, 166),
            },
            ThemeChoice::WarmLight => ThemePalette {
                background: rgb(244, 237, 225),
                window: rgb(252, 248, 241),
                accent: rgb(190, 112, 67),
            },
            ThemeChoice::Custom => {
                let background = Color32::from_rgb(
                    self.custom_background[0],
                    self.custom_background[1],
                    self.custom_background[2],
                );
                let window_tint = if uses_light_text(background) {
                    Color32::BLACK
                } else {
                    Color32::WHITE
                };
                ThemePalette {
                    background,
                    window: blend(background, window_tint, 0.08),
                    accent: Color32::from_rgb(
                        self.custom_accent[0],
                        self.custom_accent[1],
                        self.custom_accent[2],
                    ),
                }
            }
        }
    }
}

fn color_luminance(color: egui::Color32) -> f32 {
    fn linear_channel(channel: u8) -> f32 {
        let value = f32::from(channel) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }

    0.2126 * linear_channel(color.r())
        + 0.7152 * linear_channel(color.g())
        + 0.0722 * linear_channel(color.b())
}

fn readable_text_color(background: egui::Color32) -> egui::Color32 {
    if uses_light_text(background) {
        egui::Color32::from_rgb(248, 249, 252)
    } else {
        egui::Color32::from_rgb(25, 28, 33)
    }
}

fn uses_light_text(background: egui::Color32) -> bool {
    let background_luminance = color_luminance(background);
    let dark_text_luminance = color_luminance(egui::Color32::from_rgb(25, 28, 33));
    let light_text_luminance = color_luminance(egui::Color32::from_rgb(248, 249, 252));
    let contrast_ratio = |text_luminance: f32| {
        let lighter = background_luminance.max(text_luminance);
        let darker = background_luminance.min(text_luminance);
        (lighter + 0.05) / (darker + 0.05)
    };

    contrast_ratio(light_text_luminance) > contrast_ratio(dark_text_luminance)
}

fn blend(from: egui::Color32, to: egui::Color32, amount: f32) -> egui::Color32 {
    let mix = |left: u8, right: u8| {
        (f32::from(left) + (f32::from(right) - f32::from(left)) * amount)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    egui::Color32::from_rgb(
        mix(from.r(), to.r()),
        mix(from.g(), to.g()),
        mix(from.b(), to.b()),
    )
}

struct ClickerApp {
    hours: u32,
    minutes: u32,
    seconds: u32,
    milliseconds: u32,
    random_offset_enabled: bool,
    random_offset_ms: u32,
    button: ClickButton,
    click_type: ClickType,
    repeat_mode: RepeatMode,
    repeat_count: u64,
    cursor_mode: CursorMode,
    cursor_x: i32,
    cursor_y: i32,
    hotkey: Hotkey,
    hotkey_enabled: bool,
    hotkey_down: bool,
    hotkey_last_toggled: Option<Instant>,
    show_hotkey_settings: bool,
    show_recorder: bool,
    show_settings: bool,
    settings: AppSettings,
    applied_always_on_top: Option<bool>,
    settings_focus_pending: bool,
    update_status: String,
    update_check_rx: Option<Receiver<Result<UpdateCheckResult, String>>>,
    update_install_rx: Option<Receiver<Result<UpdateInstallResult, String>>>,
    available_update: Option<String>,
    update_prompt_open: bool,
    next_auto_update_check: Option<Instant>,
    recording_since: Option<SystemTime>,
    recording_armed_at: Option<SystemTime>,
    recording_wait_release: bool,
    pick_armed_at: Option<SystemTime>,
    pick_wait_release: bool,
    recorded: Vec<RecordedClick>,
    last_pointer: (i32, i32),
    listener_rx: Receiver<ListenerMessage>,
    listener_error: Option<String>,
    worker: Option<Worker>,
    last_run_clicks: u64,
    status: String,
}

impl ClickerApp {
    fn new(storage: Option<&dyn eframe::Storage>) -> Self {
        let settings: AppSettings = storage
            .and_then(|storage| eframe::get_value(storage, SETTINGS_STORAGE_KEY))
            .unwrap_or_default();
        let next_auto_update_check = if settings.auto_update_enabled && update_checks_supported() {
            Some(Instant::now())
        } else {
            None
        };
        let (listener_tx, listener_rx) = mpsc::channel();
        let listener_spawn = thread::Builder::new()
            .name("clicksmith-input-listener".to_owned())
            .spawn(move || {
                let callback_tx = listener_tx.clone();
                if let Err(error) = rdev::listen(move |event| {
                    let _ = callback_tx.send(ListenerMessage::Event(event));
                }) {
                    let _ = listener_tx.send(ListenerMessage::Error(format!("{error:?}")));
                }
            });
        let listener_error = listener_spawn
            .err()
            .map(|error| format!("Could not start the global input listener: {error}"));
        let update_status = if cfg!(debug_assertions) {
            "Update checks are disabled in debug builds.".to_owned()
        } else if settings.auto_update_enabled {
            "Automatic update checks are enabled; Clicksmith checks hourly.".to_owned()
        } else if !in_app_update_supported() {
            "Check for updates here; install them with your installer or package manager."
                .to_owned()
        } else {
            "Select Check for updates to look for a release.".to_owned()
        };
        let status = listener_error.clone().unwrap_or_else(|| "Ready".to_owned());

        Self {
            hours: settings.hours,
            minutes: settings.minutes,
            seconds: settings.seconds,
            milliseconds: settings.milliseconds,
            random_offset_enabled: settings.random_offset_enabled,
            random_offset_ms: settings.random_offset_ms,
            button: settings.button,
            click_type: settings.click_type,
            repeat_mode: settings.repeat_mode,
            repeat_count: settings.repeat_count,
            cursor_mode: settings.cursor_mode,
            cursor_x: settings.cursor_x,
            cursor_y: settings.cursor_y,
            hotkey: settings.hotkey,
            hotkey_enabled: settings.hotkey_enabled,
            hotkey_down: false,
            hotkey_last_toggled: None,
            show_hotkey_settings: false,
            show_recorder: false,
            show_settings: false,
            settings,
            applied_always_on_top: None,
            settings_focus_pending: false,
            update_status,
            update_check_rx: None,
            update_install_rx: None,
            available_update: None,
            update_prompt_open: false,
            next_auto_update_check,
            recording_since: None,
            recording_armed_at: None,
            recording_wait_release: false,
            pick_armed_at: None,
            pick_wait_release: false,
            recorded: Vec::new(),
            last_pointer: (0, 0),
            listener_rx,
            listener_error,
            worker: None,
            last_run_clicks: 0,
            status,
        }
    }

    fn interval(&self) -> Duration {
        let total_ms = u64::from(self.hours) * 3_600_000
            + u64::from(self.minutes) * 60_000
            + u64::from(self.seconds) * 1_000
            + u64::from(self.milliseconds);
        Duration::from_millis(total_ms.max(1))
    }

    fn current_click_settings(&self) -> ClickSettings {
        ClickSettings {
            interval: self.interval(),
            random_offset_ms: if self.random_offset_enabled {
                u64::from(self.random_offset_ms)
            } else {
                0
            },
            button: self.button,
            click_type: self.click_type,
            repeat_mode: self.repeat_mode,
            repeat_count: self.repeat_count.max(1),
            cursor_mode: self.cursor_mode,
            x: self.cursor_x,
            y: self.cursor_y,
        }
    }

    fn toggle_clicker(&mut self) {
        if self.worker.is_some() {
            self.stop_worker("Stopped");
            return;
        }
        if self.recording_since.is_some() {
            self.status = "Stop recording before starting the clicker".to_owned();
            return;
        }

        self.start_worker(RunPlan::Clicker(self.current_click_settings()), false);
    }

    fn start_playback(&mut self) {
        if self.worker.is_some() || self.recording_since.is_some() || self.recorded.is_empty() {
            return;
        }
        self.start_worker(RunPlan::Playback(self.recorded.clone()), true);
    }

    fn start_worker(&mut self, plan: RunPlan, playback: bool) {
        match Worker::start(plan) {
            Ok(worker) => {
                self.last_run_clicks = 0;
                self.worker = Some(worker);
                self.status = if playback {
                    "Playing recorded clicks".to_owned()
                } else {
                    "Clicking — move the pointer to the target".to_owned()
                };
            }
            Err(error) => self.status = error,
        }
    }

    fn stop_worker(&mut self, reason: &str) {
        if let Some(worker) = self.worker.take() {
            self.last_run_clicks = worker.click_count();
            worker.request_stop();
        }
        self.status = reason.to_owned();
    }

    fn update_worker(&mut self) {
        let finished = self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.thread.is_finished());
        if !finished {
            return;
        }

        if let Some(worker) = self.worker.take() {
            self.last_run_clicks = worker.click_count();
            let result = worker.result.try_recv().ok().flatten();
            let joined = worker.thread.join();
            self.status = match result {
                Some(error) => error,
                None if joined.is_err() => "The click worker stopped unexpectedly".to_owned(),
                None => "Finished".to_owned(),
            };
        }
    }

    fn start_recording(&mut self) {
        if self.worker.is_some() {
            return;
        }
        let now = SystemTime::now();
        self.recorded.clear();
        self.recording_since = Some(now);
        self.recording_armed_at = None;
        self.recording_wait_release = true;
        self.status = "Recording mouse clicks — press Esc to finish".to_owned();
    }

    fn stop_recording(&mut self) {
        self.recording_since = None;
        self.recording_armed_at = None;
        self.recording_wait_release = false;
        self.status = format!("Recorded {} clicks", self.recorded.len());
    }

    fn handle_global_event(&mut self, event: Event) {
        match event.event_type {
            EventType::MouseMove { x, y } => {
                self.last_pointer = (x.round() as i32, y.round() as i32);
            }
            EventType::ButtonPress(button) => {
                let (x, y) = self.last_pointer;

                if self
                    .pick_armed_at
                    .is_some_and(|armed_at| event.time > armed_at)
                    && !self.pick_wait_release
                {
                    self.cursor_mode = CursorMode::Fixed;
                    self.cursor_x = x;
                    self.cursor_y = y;
                    self.pick_armed_at = None;
                    self.status = format!("Picked location: X {x}, Y {y}");
                }

                if let (Some(started_at), Some(button)) =
                    (self.recording_since, ClickButton::from_global(button))
                {
                    let after_arm = self
                        .recording_armed_at
                        .is_none_or(|armed_at| event.time > armed_at);
                    if after_arm && !self.recording_wait_release {
                        let at = event.time.duration_since(started_at).unwrap_or_default();
                        self.recorded.push(RecordedClick { at, x, y, button });
                        self.status = format!("Recording — {} clicks", self.recorded.len());
                    }
                }
            }
            EventType::KeyPress(key) => {
                if key == Key::Escape && self.recording_since.is_some() {
                    self.stop_recording();
                }
                if self.hotkey_enabled && key == self.hotkey.key() && !self.hotkey_down {
                    self.hotkey_down = true;
                    let now = Instant::now();
                    let debounced = self
                        .hotkey_last_toggled
                        .is_none_or(|last| now.duration_since(last) >= HOTKEY_DEBOUNCE);
                    if debounced {
                        self.hotkey_last_toggled = Some(now);
                        self.toggle_clicker();
                    }
                }
            }
            EventType::KeyRelease(_) => {
                self.hotkey_down = false;
            }
            EventType::ButtonRelease(_) => {
                if self.recording_wait_release {
                    self.recording_wait_release = false;
                    self.recording_since = Some(event.time);
                    self.recording_armed_at = Some(event.time);
                }
                if self.pick_wait_release {
                    self.pick_wait_release = false;
                    self.pick_armed_at = Some(event.time);
                }
            }
            EventType::Wheel { .. } => {}
        }
    }

    fn handle_listener_events(&mut self) {
        while let Ok(message) = self.listener_rx.try_recv() {
            match message {
                ListenerMessage::Event(event) => self.handle_global_event(event),
                ListenerMessage::Error(error) => {
                    self.listener_error = Some(error.clone());
                    self.status = format!("Global input listener unavailable: {error}");
                }
            }
        }
    }

    fn run_click_count(&self) -> u64 {
        self.worker
            .as_ref()
            .map_or(self.last_run_clicks, Worker::click_count)
    }

    fn start_update_check(&mut self) {
        if self.update_check_rx.is_some() || self.update_install_rx.is_some() {
            return;
        }

        self.update_prompt_open = false;
        let channel = self.settings.update_channel;
        let (result_tx, result_rx) = mpsc::channel();
        let spawn_result = thread::Builder::new()
            .name("clicksmith-update-check".to_owned())
            .spawn(move || {
                let _ = result_tx.send(check_for_updates(channel));
            });

        match spawn_result {
            Ok(_) => {
                self.available_update = None;
                self.update_check_rx = Some(result_rx);
                self.update_status = "Checking GitHub for updates…".to_owned();
            }
            Err(error) => {
                self.update_status = format!("Could not start update check: {error}");
            }
        }
    }

    fn start_update_install(&mut self) {
        let Some(version) = self.available_update.clone() else {
            return;
        };
        if self.update_check_rx.is_some() || self.update_install_rx.is_some() {
            return;
        }

        let (result_tx, result_rx) = mpsc::channel();
        let install_version = version.clone();
        let channel = self.settings.update_channel;
        let spawn_result = thread::Builder::new()
            .name("clicksmith-update-install".to_owned())
            .spawn(move || {
                let _ = result_tx.send(install_update(&install_version, channel));
            });

        match spawn_result {
            Ok(_) => {
                self.update_install_rx = Some(result_rx);
                self.update_prompt_open = false;
                self.update_status = format!("Installing Clicksmith {version}…");
            }
            Err(error) => {
                self.update_status = format!("Could not start update installation: {error}");
            }
        }
    }

    fn poll_update_check(&mut self) {
        let result = match self.update_check_rx.as_ref() {
            Some(receiver) => match receiver.try_recv() {
                Ok(result) => result,
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Err("Update check stopped unexpectedly".to_owned())
                }
            },
            None => return,
        };
        self.update_check_rx = None;

        match result {
            Ok(UpdateCheckResult::UpdateAvailable(version)) => {
                self.update_status = if in_app_update_supported() {
                    if self.settings.auto_update_enabled {
                        format!("Version {version} is available. Choose Install now or Later.")
                    } else {
                        format!(
                            "Version {version} is available. Select Install update to continue."
                        )
                    }
                } else if cfg!(feature = "package-managed") {
                    format!("Version {version} is available. Open the download page to update.")
                } else if self.settings.auto_update_enabled {
                    format!("Version {version} is available. Choose Install now or Later.")
                } else {
                    format!("Version {version} is available. Select Install update to continue.")
                };
                self.update_prompt_open = self.settings.auto_update_enabled;
                self.available_update = Some(version);
            }
            Ok(UpdateCheckResult::UpToDate) => {
                self.update_status = "Clicksmith is up to date.".to_owned();
                self.available_update = None;
            }
            Ok(UpdateCheckResult::DevelopmentBuild) => {
                self.update_status =
                    "Update checks are disabled in debug builds; use a release build.".to_owned();
                self.available_update = None;
            }
            Err(error) => {
                self.update_status = format!("Update check failed: {error}");
                self.available_update = None;
            }
        }
    }

    fn poll_update_install(&mut self) {
        let result = match self.update_install_rx.as_ref() {
            Some(receiver) => match receiver.try_recv() {
                Ok(result) => result,
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Err("Update installation stopped unexpectedly".to_owned())
                }
            },
            None => return,
        };
        self.update_install_rx = None;

        match result {
            Ok(UpdateInstallResult::Updated) => {
                self.update_status = "Update installed. Restarting Clicksmith…".to_owned();
                self.status = "Stopping for update…".to_owned();
                if let Some(worker) = self.worker.take() {
                    worker.request_stop();
                    let _ = worker.thread.join();
                }
                self.recording_since = None;
                self.recording_armed_at = None;
                self.recording_wait_release = false;

                match self_update::restart::restart() {
                    Ok(_) => std::process::exit(0),
                    Err(error) => {
                        self.update_status =
                            format!("Update installed, but restart failed: {error}");
                    }
                }
            }
            #[cfg(all(target_os = "windows", feature = "windows-nsis"))]
            Ok(UpdateInstallResult::InstallerStarted) => {
                self.update_status =
                    "Downloaded the update. Closing Clicksmith to install it…".to_owned();
                self.status = "Stopping for update…".to_owned();
                if let Some(worker) = self.worker.take() {
                    worker.request_stop();
                    let _ = worker.thread.join();
                }
                self.recording_since = None;
                self.recording_armed_at = None;
                self.recording_wait_release = false;
                std::process::exit(0);
            }
            Ok(UpdateInstallResult::UpToDate) => {
                self.available_update = None;
                self.update_status = "Clicksmith is already up to date.".to_owned();
            }
            Err(error) => {
                self.update_status = format!("Update installation failed: {error}");
            }
        }
    }

    fn check_scheduled_updates(&mut self) {
        if !self.settings.auto_update_enabled || !update_checks_supported() {
            self.next_auto_update_check = None;
            return;
        }

        let check_due = self
            .next_auto_update_check
            .map_or(true, |next_check| Instant::now() >= next_check);
        if !check_due {
            return;
        }

        self.next_auto_update_check = Some(Instant::now() + AUTO_UPDATE_CHECK_INTERVAL);
        self.start_update_check();
    }

    fn show_update_prompt(&mut self, ctx: &egui::Context) {
        let Some(version) = self.available_update.clone() else {
            self.update_prompt_open = false;
            return;
        };
        if !self.update_prompt_open {
            return;
        }

        let mut open = true;
        let mut install_now = false;
        let mut open_download_page = false;
        let mut install_later = false;
        let can_install_in_app = in_app_update_supported();
        egui::Window::new("Update available")
            .id(egui::Id::new("update-available"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!("Clicksmith {version} is available."));
                ui.label(if can_install_in_app {
                    "Install now to download it and restart Clicksmith, or choose Later."
                } else {
                    "Open the release page to download the update, or choose Later."
                });
                ui.horizontal(|ui| {
                    if can_install_in_app {
                        if ui.button("Install now").clicked() {
                            install_now = true;
                        }
                    } else if ui.button("Open download page").clicked() {
                        open_download_page = true;
                    }
                    if ui.button("Later").clicked() {
                        install_later = true;
                    }
                });
            });

        self.update_prompt_open = open && !install_now && !install_later && !open_download_page;
        if open_download_page {
            self.update_status = format!("Opened the download page for version {version}.");
        } else if install_later || !open {
            self.update_status = if can_install_in_app {
                format!("Version {version} is available. You can install it from Settings.")
            } else {
                format!(
                    "Version {version} is available. You can open its download page from Settings."
                )
            };
        }
        if install_now {
            self.start_update_install();
        }
        if open_download_page {
            ctx.open_url(egui::OpenUrl::new_tab(release_page_url(&version)));
        }
    }

    fn apply_theme(&self, ctx: &egui::Context) {
        let palette = self.settings.palette();
        let text = readable_text_color(palette.background);
        let dark = uses_light_text(palette.background);
        let mut visuals = if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };

        visuals.panel_fill = palette.background;
        visuals.window_fill = palette.window;
        visuals.extreme_bg_color = egui::Color32::WHITE;
        visuals.faint_bg_color = blend(palette.background, palette.window, 0.3);
        visuals.code_bg_color = blend(palette.background, palette.window, 0.6);
        visuals.override_text_color = Some(text);
        visuals.hyperlink_color = text;
        visuals.selection.bg_fill = blend(palette.background, palette.window, 0.5);
        visuals.selection.stroke = egui::Stroke::new(1.0_f32, palette.accent);
        visuals.window_stroke.color = blend(palette.background, palette.accent, 0.35);
        visuals.button_frame = true;
        visuals.window_shadow = egui::epaint::Shadow {
            offset: [0, 3],
            blur: 8,
            spread: 1,
            color: egui::Color32::from_black_alpha(70),
        };
        visuals.popup_shadow = egui::epaint::Shadow {
            offset: [0, 3],
            blur: 6,
            spread: 1,
            color: egui::Color32::from_black_alpha(90),
        };

        let quiet_fill = blend(palette.background, palette.window, 0.55);
        let accent_text = readable_text_color(palette.accent);
        let hover_fill = blend(palette.accent, egui::Color32::WHITE, 0.12);
        let active_fill = blend(palette.accent, egui::Color32::BLACK, 0.12);
        for widget in [
            &mut visuals.widgets.noninteractive,
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widget.fg_stroke.color = accent_text;
            widget.corner_radius = egui::CornerRadius::same(6);
        }
        visuals.widgets.noninteractive.bg_fill = quiet_fill;
        visuals.widgets.noninteractive.weak_bg_fill = palette.background;
        visuals.widgets.noninteractive.bg_stroke =
            egui::Stroke::new(1.0_f32, blend(palette.background, palette.accent, 0.2));
        visuals.widgets.inactive.bg_fill = palette.accent;
        visuals.widgets.inactive.weak_bg_fill = palette.background;
        visuals.widgets.inactive.bg_stroke =
            egui::Stroke::new(1.0_f32, blend(palette.accent, egui::Color32::WHITE, 0.28));
        visuals.widgets.inactive.expansion = 1.5;
        visuals.widgets.hovered.bg_fill = hover_fill;
        visuals.widgets.hovered.fg_stroke.color = readable_text_color(hover_fill);
        visuals.widgets.hovered.weak_bg_fill = blend(palette.background, palette.window, 0.4);
        visuals.widgets.hovered.bg_stroke =
            egui::Stroke::new(1.5_f32, blend(hover_fill, egui::Color32::WHITE, 0.35));
        visuals.widgets.hovered.expansion = 2.5;
        visuals.widgets.active.bg_fill = active_fill;
        visuals.widgets.active.fg_stroke.color = readable_text_color(active_fill);
        visuals.widgets.active.weak_bg_fill = blend(palette.background, palette.window, 0.6);
        visuals.widgets.active.bg_stroke =
            egui::Stroke::new(1.5_f32, blend(active_fill, egui::Color32::BLACK, 0.28));
        visuals.widgets.active.expansion = -1.0;
        visuals.widgets.open.bg_fill = hover_fill;
        visuals.widgets.open.fg_stroke.color = readable_text_color(hover_fill);
        visuals.widgets.open.weak_bg_fill = blend(palette.background, palette.window, 0.4);
        visuals.widgets.open.bg_stroke =
            egui::Stroke::new(1.5_f32, blend(hover_fill, egui::Color32::WHITE, 0.35));
        visuals.widgets.open.expansion = 2.0;

        ctx.style_mut(|style| {
            style.spacing.button_padding = egui::vec2(12.0, 5.0);
            style.visuals = visuals;
        });
    }

    fn apply_window_level(&mut self, ctx: &egui::Context) {
        if self.applied_always_on_top == Some(self.settings.always_on_top) {
            return;
        }

        let level = if self.settings.always_on_top {
            egui::WindowLevel::AlwaysOnTop
        } else {
            egui::WindowLevel::Normal
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
        self.applied_always_on_top = Some(self.settings.always_on_top);
    }

    fn show_settings_page(&mut self, ui: &mut egui::Ui) -> bool {
        let mut request_update_check = false;
        let mut request_update_install = false;
        let mut request_open_download_page = false;
        let mut close_requested = false;
        ui.add_space(6.0);
        ui.heading("Settings");
        ui.add_space(8.0);
        ui.label("Appearance");
        ui.horizontal(|ui| {
            ui.label("Theme");
            egui::ComboBox::from_id_salt("theme-choice")
                .selected_text(self.settings.theme.label())
                .show_ui(ui, |ui| {
                    for theme in ThemeChoice::ALL {
                        ui.selectable_value(&mut self.settings.theme, theme, theme.label());
                    }
                });
        });

        if self.settings.theme == ThemeChoice::Custom {
            let mut background = egui::Color32::from_rgb(
                self.settings.custom_background[0],
                self.settings.custom_background[1],
                self.settings.custom_background[2],
            );
            let mut accent = egui::Color32::from_rgb(
                self.settings.custom_accent[0],
                self.settings.custom_accent[1],
                self.settings.custom_accent[2],
            );
            ui.horizontal(|ui| {
                ui.label("Background");
                ui.color_edit_button_srgba(&mut background);
            });
            ui.horizontal(|ui| {
                ui.label("Accent");
                ui.color_edit_button_srgba(&mut accent);
            });
            self.settings.custom_background = [background.r(), background.g(), background.b()];
            self.settings.custom_accent = [accent.r(), accent.g(), accent.b()];
        }

        ui.label("Text color adjusts automatically for contrast.");
        let always_on_top_changed = ui
            .checkbox(
                &mut self.settings.always_on_top,
                "Keep main window always on top",
            )
            .changed();
        if always_on_top_changed && self.settings.always_on_top {
            self.settings_focus_pending = true;
        }
        ui.separator();
        ui.label("Updates");
        ui.label(format!("Current version: {}", env!("CARGO_PKG_VERSION")));
        let update_supported = update_checks_supported();
        let update_in_progress = self.update_check_rx.is_some() || self.update_install_rx.is_some();
        let previous_update_channel = self.settings.update_channel;
        ui.add_enabled_ui(!update_in_progress, |ui| {
            ui.horizontal(|ui| {
                ui.label("Update channel");
                egui::ComboBox::from_id_salt("update-channel")
                    .selected_text(self.settings.update_channel.label())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.settings.update_channel,
                            UpdateChannel::Stable,
                            UpdateChannel::Stable.label(),
                        );
                        ui.selectable_value(
                            &mut self.settings.update_channel,
                            UpdateChannel::Beta,
                            UpdateChannel::Beta.label(),
                        );
                    });
            });
        });
        if self.settings.update_channel != previous_update_channel {
            self.available_update = None;
            self.update_prompt_open = false;
            if self.settings.auto_update_enabled && update_supported {
                self.next_auto_update_check = Some(Instant::now());
                self.update_status = format!(
                    "{} channel selected. Checking for updates now.",
                    self.settings.update_channel.label()
                );
            } else if update_supported {
                self.update_status = format!(
                    "{} channel selected. Check for updates to apply it.",
                    self.settings.update_channel.label()
                );
            } else {
                self.update_status =
                    "Update channel selection takes effect in release builds.".to_owned();
            }
        }
        ui.small("Stable gets stable releases only. Beta also gets stable releases.");
        ui.add_enabled_ui(update_supported, |ui| {
            if ui
                .checkbox(
                    &mut self.settings.auto_update_enabled,
                    "Automatically check every hour (prompt before updating)",
                )
                .changed()
            {
                if self.settings.auto_update_enabled {
                    self.next_auto_update_check = Some(Instant::now());
                    self.update_status =
                        "Automatic checks enabled. Checking now and then hourly.".to_owned();
                } else {
                    self.next_auto_update_check = None;
                    self.update_status = "Automatic update checks are off.".to_owned();
                }
            }
        });
        if cfg!(feature = "package-managed") && !in_app_update_supported() {
            ui.small("Checks are available; downloads and installation are handled externally.");
        } else if !update_supported {
            ui.small("Automatic updates are available in release builds only.");
        }
        let can_install_in_app = in_app_update_supported();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !update_in_progress && update_supported,
                    egui::Button::new(if self.update_check_rx.is_some() {
                        "Checking for updates…"
                    } else {
                        "Check for updates"
                    }),
                )
                .clicked()
            {
                request_update_check = true;
            }
            if self.available_update.is_some() || self.update_install_rx.is_some() {
                if ui
                    .add_enabled(
                        self.available_update.is_some() && !update_in_progress,
                        egui::Button::new(if self.update_install_rx.is_some() {
                            "Installing…"
                        } else if can_install_in_app {
                            "Install update"
                        } else {
                            "Open download page"
                        }),
                    )
                    .clicked()
                {
                    if can_install_in_app {
                        request_update_install = true;
                    } else {
                        request_open_download_page = true;
                    }
                }
            }
        });
        ui.label(&self.update_status);
        ui.add_space(8.0);
        if ui.button("Close Settings").clicked() {
            close_requested = true;
        }

        if request_update_check {
            self.start_update_check();
        }
        if request_update_install {
            self.start_update_install();
        }
        if request_open_download_page {
            if let Some(version) = self.available_update.as_deref() {
                ui.ctx()
                    .open_url(egui::OpenUrl::new_tab(release_page_url(version)));
            }
        }
        close_requested
    }

    fn show_settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }

        let viewport_id = egui::ViewportId::from_hash_of("settings");
        let viewport_builder = egui::ViewportBuilder::default()
            .with_title("Clicksmith — Settings")
            .with_inner_size([480.0, 520.0])
            .with_min_inner_size([400.0, 360.0])
            .with_window_level(if self.settings.always_on_top {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            });
        let close_requested = ctx.show_viewport_immediate(
            viewport_id,
            viewport_builder,
            |settings_ctx, viewport_class| {
                if self.settings_focus_pending {
                    if viewport_class != egui::ViewportClass::Embedded {
                        settings_ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    self.settings_focus_pending = false;
                }
                let close_clicked = if viewport_class == egui::ViewportClass::Embedded {
                    let mut open = true;
                    let mut close_clicked = false;
                    egui::Window::new("Clicksmith — Settings")
                        .open(&mut open)
                        .show(settings_ctx, |ui| {
                            close_clicked = self.show_settings_page(ui);
                        });
                    close_clicked || !open
                } else {
                    let mut close_clicked = false;
                    egui::CentralPanel::default().show(settings_ctx, |ui| {
                        close_clicked = self.show_settings_page(ui);
                    });
                    close_clicked
                };
                if close_clicked {
                    settings_ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                close_clicked || settings_ctx.input(|input| input.viewport().close_requested())
            },
        );
        if close_requested {
            self.show_settings = false;
        }
    }

    fn show_hotkey_window(&mut self, ctx: &egui::Context) {
        egui::Window::new("Hotkey settings")
            .open(&mut self.show_hotkey_settings)
            .resizable(false)
            .show(ctx, |ui| {
                ui.checkbox(&mut self.hotkey_enabled, "Enable global start/stop hotkey");
                ui.horizontal(|ui| {
                    ui.label("Toggle key");
                    egui::ComboBox::from_id_salt("hotkey-choice")
                        .selected_text(self.hotkey.label())
                        .show_ui(ui, |ui| {
                            for key in [
                                Hotkey::F6,
                                Hotkey::F7,
                                Hotkey::F8,
                                Hotkey::F9,
                                Hotkey::F10,
                                Hotkey::F11,
                                Hotkey::F12,
                            ] {
                                ui.selectable_value(&mut self.hotkey, key, key.label());
                            }
                        });
                });
                ui.label("The selected key toggles clicking while this app is in the background.");
                if self.listener_error.is_some() {
                    ui.colored_label(
                        egui::Color32::LIGHT_RED,
                        "Global hotkeys need the input listener, which is unavailable on this session.",
                    );
                }
            });
    }

    fn show_recorder_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_recorder;
        egui::Window::new("Record & Playback")
            .open(&mut open)
            .resizable(false)
            .default_width(360.0)
            .show(ctx, |ui| {
                ui.label(format!("{} clicks in the sequence", self.recorded.len()));
                ui.add_space(4.0);

                if self.recording_since.is_some() {
                    ui.colored_label(egui::Color32::LIGHT_RED, "● Recording");
                    if ui.button("Stop recording").clicked() {
                        self.stop_recording();
                    }
                    ui.small("You can also press Esc to finish recording.");
                } else if ui
                    .add_enabled(self.worker.is_none(), egui::Button::new("Record clicks"))
                    .clicked()
                {
                    self.start_recording();
                }

                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !self.recorded.is_empty()
                                && self.worker.is_none()
                                && self.recording_since.is_none(),
                            egui::Button::new("Play sequence"),
                        )
                        .clicked()
                    {
                        self.start_playback();
                    }

                    if ui
                        .add_enabled(self.worker.is_some(), egui::Button::new("Stop playback"))
                        .clicked()
                    {
                        self.stop_worker("Playback stopped");
                    }

                    if ui
                        .add_enabled(
                            !self.recorded.is_empty() && self.recording_since.is_none(),
                            egui::Button::new("Clear"),
                        )
                        .clicked()
                    {
                        self.recorded.clear();
                        self.status = "Recording cleared".to_owned();
                    }
                });

                ui.small("Playback restores the recorded click positions and timing.");
            });
        self.show_recorder = open;
    }
}

impl eframe::App for ClickerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.update_worker();
        self.poll_update_check();
        self.poll_update_install();
        self.check_scheduled_updates();
        self.apply_theme(ctx);
        self.apply_window_level(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(6.0);
            ui.vertical_centered(|ui| {
                ui.heading("Clicksmith");
                ui.label("Cross-platform mouse automation");
                ui.small(format!("Version {}", env!("CARGO_PKG_VERSION")));
            });
            ui.add_space(12.0);

            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label("Click interval");
                ui.horizontal_wrapped(|ui| {
                    interval_field(ui, "Hours", &mut self.hours, 999);
                    interval_field(ui, "Mins", &mut self.minutes, 59);
                    interval_field(ui, "Secs", &mut self.seconds, 59);
                    interval_field(ui, "Millis", &mut self.milliseconds, 999);
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.random_offset_enabled, "Random offset ±");
                    ui.add_enabled(
                        self.random_offset_enabled,
                        egui::DragValue::new(&mut self.random_offset_ms)
                            .range(0..=60_000)
                            .suffix(" ms"),
                    );
                });
            });

            ui.add_space(8.0);
            ui.columns(2, |columns| {
                egui::Frame::group(columns[0].style()).show(&mut columns[0], |ui| {
                    ui.label("Click options");
                    egui::ComboBox::from_id_salt("mouse-button")
                        .selected_text(self.button.label())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.button, ClickButton::Left, "Left");
                            ui.selectable_value(&mut self.button, ClickButton::Right, "Right");
                            ui.selectable_value(&mut self.button, ClickButton::Middle, "Middle");
                        });
                    egui::ComboBox::from_id_salt("click-type")
                        .selected_text(self.click_type.label())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.click_type, ClickType::Single, "Single");
                            ui.selectable_value(&mut self.click_type, ClickType::Double, "Double");
                            ui.selectable_value(&mut self.click_type, ClickType::Triple, "Triple");
                        });
                });

                egui::Frame::group(columns[1].style()).show(&mut columns[1], |ui| {
                    ui.label("Click repeat");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.repeat_mode, RepeatMode::Count, "Repeat");
                        ui.add_enabled_ui(self.repeat_mode == RepeatMode::Count, |ui| {
                            ui.add(egui::DragValue::new(&mut self.repeat_count).range(1..=999_999));
                            ui.label("times");
                        });
                    });
                    ui.radio_value(
                        &mut self.repeat_mode,
                        RepeatMode::UntilStopped,
                        "Repeat until stopped",
                    );
                });
            });

            ui.add_space(8.0);
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label("Cursor position");
                ui.horizontal_wrapped(|ui| {
                    ui.radio_value(
                        &mut self.cursor_mode,
                        CursorMode::Current,
                        "Current location",
                    );
                    ui.radio_value(&mut self.cursor_mode, CursorMode::Fixed, "Fixed location");
                    if ui
                        .add_enabled(self.worker.is_none(), egui::Button::new("Pick location"))
                        .clicked()
                    {
                        self.pick_armed_at = None;
                        self.pick_wait_release = true;
                        self.status = "Click any screen location to pick it".to_owned();
                    }
                });
                if self.cursor_mode == CursorMode::Fixed {
                    ui.horizontal(|ui| {
                        ui.label("X");
                        ui.add(egui::DragValue::new(&mut self.cursor_x).range(-99_999..=99_999));
                        ui.label("Y");
                        ui.add(egui::DragValue::new(&mut self.cursor_y).range(-99_999..=99_999));
                    });
                }
                if self.pick_armed_at.is_some() {
                    ui.small("Move the pointer and click the target location.");
                }
            });

            ui.add_space(20.0);
            ui.horizontal(|ui| {
                ui.add_enabled_ui(
                    self.worker.is_none() && self.recording_since.is_none(),
                    |ui| {
                        if ui
                            .add_sized(
                                [190.0, 38.0],
                                egui::Button::new(format!("Start ({})", self.hotkey.label())),
                            )
                            .clicked()
                        {
                            self.start_worker(
                                RunPlan::Clicker(self.current_click_settings()),
                                false,
                            );
                        }
                    },
                );
                if ui
                    .add_enabled(
                        self.worker.is_some(),
                        egui::Button::new(format!("Stop ({})", self.hotkey.label()))
                            .min_size(egui::vec2(190.0, 38.0)),
                    )
                    .clicked()
                {
                    self.stop_worker("Stopped");
                }
            });

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Hotkey setting").clicked() {
                    self.show_hotkey_settings = true;
                }
                if ui.button("Record & Playback").clicked() {
                    self.show_recorder = true;
                }
                if ui.button("Settings").clicked() {
                    self.show_settings = true;
                }
            });

            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("Clicks: {}", self.run_click_count()));
                ui.separator();
                ui.label(&self.status);
            });
            if self.listener_error.is_some() {
                ui.colored_label(
                    egui::Color32::LIGHT_RED,
                    "Global controls unavailable. Check input permissions or use the buttons.",
                );
            }
            ui.add_space(4.0);
            ui.small("The first click happens immediately. Current location follows your pointer.");
        });

        self.show_settings_window(ctx);
        self.show_hotkey_window(ctx);
        self.show_recorder_window(ctx);
        self.show_update_prompt(ctx);
        self.handle_listener_events();

        ctx.request_repaint_after(UI_REFRESH);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.settings.hours = self.hours;
        self.settings.minutes = self.minutes;
        self.settings.seconds = self.seconds;
        self.settings.milliseconds = self.milliseconds;
        self.settings.random_offset_enabled = self.random_offset_enabled;
        self.settings.random_offset_ms = self.random_offset_ms;
        self.settings.button = self.button;
        self.settings.click_type = self.click_type;
        self.settings.repeat_mode = self.repeat_mode;
        self.settings.repeat_count = self.repeat_count;
        self.settings.cursor_mode = self.cursor_mode;
        self.settings.cursor_x = self.cursor_x;
        self.settings.cursor_y = self.cursor_y;
        self.settings.hotkey = self.hotkey;
        self.settings.hotkey_enabled = self.hotkey_enabled;
        eframe::set_value(storage, SETTINGS_STORAGE_KEY, &self.settings);
    }
}

impl Drop for ClickerApp {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.request_stop();
        }
    }
}

fn interval_field(ui: &mut egui::Ui, label: &str, value: &mut u32, maximum: u32) {
    ui.vertical(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(value).range(0..=maximum).speed(1));
    });
}

#[cfg(test)]
mod update_channel_tests {
    use super::{UpdateChannel, latest_eligible_version};

    #[test]
    fn stable_channel_skips_prereleases_and_selects_the_newest_stable_version() {
        let version = latest_eligible_version(
            vec![
                "0.2.0-beta.4".to_owned(),
                "0.2.0".to_owned(),
                "0.2.1".to_owned(),
                "0.3.0-beta.1".to_owned(),
            ],
            UpdateChannel::Stable,
        )
        .unwrap();

        assert_eq!(version.as_deref(), Some("0.2.1"));
    }

    #[test]
    fn beta_channel_includes_prereleases_and_stable_releases() {
        let prerelease = latest_eligible_version(
            vec![
                "0.2.0".to_owned(),
                "0.3.0-beta.2".to_owned(),
                "0.3.0-beta.4".to_owned(),
            ],
            UpdateChannel::Beta,
        )
        .unwrap();
        let stable = latest_eligible_version(
            vec!["0.3.0-beta.4".to_owned(), "0.3.0".to_owned()],
            UpdateChannel::Beta,
        )
        .unwrap();

        assert_eq!(prerelease.as_deref(), Some("0.3.0-beta.4"));
        assert_eq!(stable.as_deref(), Some("0.3.0"));
    }

    #[test]
    fn stable_channel_has_no_update_when_only_prereleases_exist() {
        let version =
            latest_eligible_version(vec!["0.3.0-beta.4".to_owned()], UpdateChannel::Stable)
                .unwrap();

        assert_eq!(version, None);
    }
}

#[cfg(all(
    test,
    feature = "package-managed",
    not(all(target_os = "windows", feature = "windows-nsis"))
))]
mod package_managed_tests {
    use super::install_package_managed_update;

    #[test]
    fn package_managed_build_without_nsis_keeps_installation_external() {
        assert_eq!(
            install_package_managed_update("1.2.3").unwrap_err(),
            "Updates for this installation are managed externally"
        );
    }
}

#[cfg(all(test, target_os = "windows", feature = "windows-nsis"))]
mod update_tests {
    use std::io::Cursor;

    use super::verify_sha256_digest;

    #[test]
    fn verifies_github_sha256_digest() {
        let mut valid = Cursor::new(b"abc");
        assert!(
            verify_sha256_digest(
                &mut valid,
                Some("sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
            )
            .is_ok()
        );

        let mut invalid = Cursor::new(b"abd");
        assert!(
            verify_sha256_digest(
                &mut invalid,
                Some("sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
            )
            .is_err()
        );
    }

    #[test]
    fn refuses_missing_or_unsupported_release_digests() {
        let mut input = Cursor::new(b"installer");
        assert!(verify_sha256_digest(&mut input, None).is_err());

        let mut input = Cursor::new(b"installer");
        assert!(verify_sha256_digest(&mut input, Some("sha1:abc")).is_err());
    }

    #[test]
    fn selects_the_windows_setup_installer_asset() {
        let mut release = self_update::Release::builder();
        release.version("1.2.3").assets([
            self_update::ReleaseAsset::new(
                "clicksmith-windows-x86_64.zip",
                "https://example.invalid/portable.zip",
            ),
            self_update::ReleaseAsset::new(
                "clicksmith-windows-x86_64-setup.exe",
                "https://example.invalid/setup.exe",
            ),
        ]);
        let release = release.build().unwrap();
        let asset = super::windows_setup_asset(&release).unwrap();
        assert_eq!(asset.name(), "clicksmith-windows-x86_64-setup.exe");
    }
}
