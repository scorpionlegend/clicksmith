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

const CLICK_GAP: Duration = Duration::from_millis(55);
const UI_REFRESH: Duration = Duration::from_millis(16);
const HOTKEY_DEBOUNCE: Duration = Duration::from_millis(300);

fn main() -> eframe::Result {
    let startup_status = match check_for_updates() {
        Ok(UpdateCheckResult::Updated) => match self_update::restart::restart() {
            Ok(_) => std::process::exit(0),
            Err(error) => Some(format!("Update installed, but restart failed: {error}")),
        },
        Ok(UpdateCheckResult::UpToDate | UpdateCheckResult::DevelopmentBuild) => None,
        Err(error) => Some(format!("Auto-update skipped: {error}")),
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([500.0, 690.0])
            .with_resizable(false),
        ..Default::default()
    };

    eframe::run_native(
        "Clicksmith",
        options,
        Box::new(move |creation_context| {
            Ok(Box::new(ClickerApp::new(
                startup_status,
                creation_context.storage,
            )))
        }),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UpdateCheckResult {
    Updated,
    UpToDate,
    DevelopmentBuild,
}

fn check_for_updates() -> Result<UpdateCheckResult, String> {
    if cfg!(debug_assertions) {
        return Ok(UpdateCheckResult::DevelopmentBuild);
    }

    let update_status = self_update::backends::github::Update::configure()
        .repo_owner("scorpionlegend")
        .repo_name("clicksmith")
        .bin_name("clicksmith")
        .asset_identifier(release_asset_identifier())
        .current_version(self_update::cargo_crate_version!())
        .update_strategy(self_update::UpdateStrategy::Latest)
        .timeout(Duration::from_secs(30))
        .check_install_path_writable(true)
        .unattended()
        .build()
        .and_then(|updater| updater.update());

    match update_status {
        Ok(status) if status.is_updated() => Ok(UpdateCheckResult::Updated),
        Ok(_) => Ok(UpdateCheckResult::UpToDate),
        Err(error) => Err(error.to_string()),
    }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RepeatMode {
    UntilStopped,
    Count,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CursorMode {
    Current,
    Fixed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    BlackPink,
    BabyBlue,
    Lavender,
    Mint,
    WarmLight,
    Custom,
}

impl ThemeChoice {
    const ALL: [Self; 7] = [
        Self::ClassicGray,
        Self::BlackPink,
        Self::BabyBlue,
        Self::Lavender,
        Self::Mint,
        Self::WarmLight,
        Self::Custom,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::ClassicGray => "Classic gray",
            Self::BlackPink => "Black pink",
            Self::BabyBlue => "Baby blue",
            Self::Lavender => "Lavender",
            Self::Mint => "Mint",
            Self::WarmLight => "Warm light",
            Self::Custom => "Custom colors",
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct AppSettings {
    theme: ThemeChoice,
    custom_background: [u8; 3],
    custom_accent: [u8; 3],
    always_on_top: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::ClassicGray,
            custom_background: [228, 231, 235],
            custom_accent: [74, 134, 202],
            always_on_top: false,
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
            ThemeChoice::BlackPink => ThemePalette {
                background: rgb(20, 18, 24),
                window: rgb(34, 29, 39),
                accent: rgb(244, 91, 171),
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
    update_status: String,
    update_check_rx: Option<Receiver<Result<UpdateCheckResult, String>>>,
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
    fn new(startup_status: Option<String>, storage: Option<&dyn eframe::Storage>) -> Self {
        let settings = storage
            .and_then(|storage| eframe::get_value(storage, SETTINGS_STORAGE_KEY))
            .unwrap_or_default();
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
        } else if let Some(status) = startup_status.as_ref() {
            status.clone()
        } else {
            "Release update check runs when Clicksmith starts.".to_owned()
        };
        let status = listener_error
            .clone()
            .or(startup_status)
            .unwrap_or_else(|| "Ready".to_owned());

        Self {
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
            hotkey_down: false,
            hotkey_last_toggled: None,
            show_hotkey_settings: false,
            show_recorder: false,
            show_settings: false,
            settings,
            applied_always_on_top: None,
            update_status,
            update_check_rx: None,
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
        if self.update_check_rx.is_some() {
            return;
        }

        let (result_tx, result_rx) = mpsc::channel();
        let spawn_result = thread::Builder::new()
            .name("clicksmith-update-check".to_owned())
            .spawn(move || {
                let _ = result_tx.send(check_for_updates());
            });

        match spawn_result {
            Ok(_) => {
                self.update_check_rx = Some(result_rx);
                self.update_status = "Checking GitHub for updates…".to_owned();
            }
            Err(error) => {
                self.update_status = format!("Could not start update check: {error}");
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
            Ok(UpdateCheckResult::UpToDate) => {
                self.update_status = "Clicksmith is up to date.".to_owned();
            }
            Ok(UpdateCheckResult::DevelopmentBuild) => {
                self.update_status =
                    "Update checks are disabled in debug builds; use a release build.".to_owned();
            }
            Ok(UpdateCheckResult::Updated) => {
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
            Err(error) => {
                self.update_status = format!("Update check failed: {error}");
            }
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
        visuals.extreme_bg_color = blend(palette.background, palette.window, 0.5);
        visuals.faint_bg_color = blend(palette.background, palette.window, 0.3);
        visuals.code_bg_color = blend(palette.background, palette.window, 0.6);
        visuals.override_text_color = Some(text);
        visuals.hyperlink_color = text;
        visuals.selection.bg_fill = blend(palette.background, palette.window, 0.5);
        visuals.selection.stroke = egui::Stroke::new(1.0_f32, palette.accent);
        visuals.window_stroke.color = blend(palette.background, palette.accent, 0.35);

        let quiet_fill = blend(palette.background, palette.window, 0.55);
        let idle_fill = blend(palette.background, palette.window, 0.75);
        let hover_fill = blend(palette.background, palette.window, 0.65);
        let active_fill = blend(palette.background, palette.window, 0.85);
        for widget in [
            &mut visuals.widgets.noninteractive,
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widget.fg_stroke.color = text;
            widget.bg_stroke.color = blend(palette.background, palette.accent, 0.2);
        }
        visuals.widgets.noninteractive.bg_fill = quiet_fill;
        visuals.widgets.noninteractive.weak_bg_fill = palette.background;
        visuals.widgets.inactive.bg_fill = idle_fill;
        visuals.widgets.inactive.weak_bg_fill = palette.background;
        visuals.widgets.hovered.bg_fill = hover_fill;
        visuals.widgets.hovered.weak_bg_fill = blend(palette.background, palette.window, 0.4);
        visuals.widgets.active.bg_fill = active_fill;
        visuals.widgets.active.weak_bg_fill = blend(palette.background, palette.window, 0.6);
        visuals.widgets.open.bg_fill = hover_fill;
        visuals.widgets.open.weak_bg_fill = blend(palette.background, palette.window, 0.4);

        ctx.set_visuals(visuals);
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

    fn show_settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_settings;
        let mut request_update_check = false;
        egui::Window::new("Settings")
            .open(&mut open)
            .resizable(false)
            .default_width(390.0)
            .show(ctx, |ui| {
                ui.heading("Appearance");
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
                    self.settings.custom_background =
                        [background.r(), background.g(), background.b()];
                    self.settings.custom_accent = [accent.r(), accent.g(), accent.b()];
                }

                ui.label("Text color adjusts automatically for contrast.");
                ui.checkbox(
                    &mut self.settings.always_on_top,
                    "Keep main window always on top",
                );
                ui.separator();
                ui.heading("Updates");
                ui.label(format!("Current version: {}", env!("CARGO_PKG_VERSION")));
                if ui
                    .add_enabled(
                        self.update_check_rx.is_none(),
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
                ui.label(&self.update_status);
            });
        self.show_settings = open;

        if request_update_check {
            self.start_update_check();
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
            ui.horizontal(|ui| {
                egui::Frame::group(ui.style()).show(ui, |ui| {
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

                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.label("Click repeat");
                    ui.radio_value(&mut self.repeat_mode, RepeatMode::Count, "Repeat");
                    ui.add_enabled_ui(self.repeat_mode == RepeatMode::Count, |ui| {
                        ui.horizontal(|ui| {
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

            ui.add_space(10.0);
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

        self.show_hotkey_window(ctx);
        self.show_recorder_window(ctx);
        self.show_settings_window(ctx);
        self.handle_listener_events();

        ctx.request_repaint_after(UI_REFRESH);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
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
