//! Asides: menu-bar app that records spoken bug reports and appends them, transcribed locally,
//! to the latest note in the directory bound with the `asides` CLI.

mod audio;
mod clipboard;
mod environment;
mod hotkeys;
mod icons;
mod macos;
mod stt;
mod tray;
mod watch;
mod worker;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use asides_core::agents::{self, Listener};
use asides_core::config::{AgentDelivery, Config};
use asides_core::link::Link;
use asides_core::models::ModelSpec;
use asides_core::notes;
use asides_core::paths::{self, tilde};
use asides_core::project;
use asides_core::state::{self as app_state, AgentSend, AppState, PendingNew};
use asides_core::wispr::{self, Dictation};
use chrono::{DateTime, Local};
use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use objc2::MainThreadMarker;
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
use tray_icon::TrayIconEvent;
use tray_icon::menu::MenuEvent;

use crate::environment::Environment;
use crate::hotkeys::{Action, Hotkeys};
use crate::icons::Icons;
use crate::macos::Mic;
use crate::stt::{ModelManager, ModelState};
use crate::tray::{MenuAction, Tray, View};
use crate::worker::{CapturedImage, DictationJob, ImageJob, Job, Outcome, RecordingJob};

/// A press shorter than this toggles recording; a longer one is push-to-talk.
const TAP_MAX: Duration = Duration::from_millis(350);
const TICK: Duration = Duration::from_millis(250);
const FLASH: Duration = Duration::from_secs(2);
const ERROR_ICON: Duration = Duration::from_secs(6);
/// Recordings shorter than this (without images) are treated as accidental and dropped.
const MIN_RECORDING_SECS: f32 = 0.3;
/// An agent that stopped waiting this recently is still listening: it's working on the last asides.
const LISTENING_GRACE_MS: u64 = 10 * 60 * 1000;

enum UserEvent {
    HotKey(GlobalHotKeyEvent),
    Menu(MenuEvent),
    Work(Outcome),
    Model(&'static str, ModelState),
    Mic(bool),
    ConfigFileChanged,
    Wispr(Result<Dictation, String>),
    Listeners(Vec<Listener>),
}

struct Recording {
    recorder: audio::Recorder,
    started_at: Instant,
    started: DateTime<Local>,
    dir: PathBuf,
    new_note: Option<Option<String>>,
    target_label: String,
    images: Vec<CapturedImage>,
    environment: Environment,
    /// The shortcut press that started it, for hold-to-talk.
    pressed: Option<(Action, Instant)>,
    clipboard: isize,
}

struct LatestNote {
    title: String,
    entries: u32,
    file: PathBuf,
}

struct App {
    mtm: MainThreadMarker,
    proxy: EventLoopProxy<UserEvent>,
    config: Config,
    icons: Option<Icons>,
    tray: Option<Tray>,
    hotkeys: Option<Hotkeys>,
    models: Arc<ModelManager>,
    jobs: Sender<Job>,
    bound: Option<PathBuf>,
    pending_new: Option<PendingNew>,
    recent_dirs: Vec<PathBuf>,
    latest: Option<LatestNote>,
    recording: Option<Recording>,
    in_flight: usize,
    model_state: Option<ModelState>,
    flash: Option<(Instant, String, String)>,
    error: Option<(Instant, String)>,
    mic: Mic,
    keys_down: HashSet<u32>,
    last_request: Option<String>,
    /// The last Send to Agent, published in `state.json` for `asides mcp`.
    agent_send: Option<AgentSend>,
    /// Send to Agent was chosen while recordings were still being transcribed.
    pending_send: bool,
    listeners: Vec<Listener>,
    /// Set to stop watching Wispr Flow's history.
    wispr: Option<Arc<AtomicBool>>,
    quit_when_idle: bool,
    quit: bool,
}

fn main() {
    let mtm = MainThreadMarker::new().expect("must start on the main thread");
    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    event_loop.set_activation_policy(ActivationPolicy::Accessory);
    // tao activates the app at launch by default; a background menu-bar app must not take focus.
    event_loop.set_activate_ignoring_other_apps(false);
    let proxy = event_loop.create_proxy();

    let p = proxy.clone();
    GlobalHotKeyEvent::set_event_handler(Some(move |e| {
        let _ = p.send_event(UserEvent::HotKey(e));
    }));
    let p = proxy.clone();
    MenuEvent::set_event_handler(Some(move |e| {
        let _ = p.send_event(UserEvent::Menu(e));
    }));
    // Unused, but without a handler every hover over the icon would queue an event forever.
    TrayIconEvent::set_event_handler(Some(|_| {}));

    let mut app = App::new(mtm, proxy);
    event_loop.run(move |event, _, control_flow| {
        if let Event::NewEvents(StartCause::ResumeTimeReached { .. }) = &event {
            app.tick();
        }
        match event {
            Event::NewEvents(StartCause::Init) => app.init(),
            Event::Opened { urls } => urls.iter().for_each(|url| app.handle_link(url)),
            Event::UserEvent(event) => app.handle(event),
            _ => {}
        }
        *control_flow = if app.quit {
            ControlFlow::Exit
        } else {
            app.next_wakeup().map_or(ControlFlow::Wait, ControlFlow::WaitUntil)
        };
    });
}

impl App {
    fn new(mtm: MainThreadMarker, proxy: EventLoopProxy<UserEvent>) -> App {
        let config_path = paths::config_path();
        let config = Config::load(&config_path).unwrap_or_default();
        if !config_path.exists() {
            let _ = config.save(&config_path);
        }
        let p = proxy.clone();
        let models = ModelManager::new(move |id, state| {
            let _ = p.send_event(UserEvent::Model(id, state));
        });
        let p = proxy.clone();
        let jobs = worker::spawn(models.clone(), move |outcome| {
            let _ = p.send_event(UserEvent::Work(outcome));
        });
        // Re-bind to where the previous session left off (the CLI usually re-binds anyway).
        let previous = AppState::read(&paths::state_path()).unwrap_or_default();
        let bound = previous.bound_dir.filter(|d| d.is_dir());
        let pending_new = bound.as_ref().and(previous.pending_new);
        let agent_send = previous.agent_send;
        let mut recent_dirs: Vec<PathBuf> = previous.recent_dirs.into_iter().filter(|d| d.is_dir()).collect();
        if let Some(dir) = &bound {
            app_state::remember_dir(&mut recent_dirs, dir);
        }
        App {
            mtm,
            proxy,
            config,
            icons: None,
            tray: None,
            hotkeys: None,
            models,
            jobs,
            bound,
            pending_new,
            recent_dirs,
            latest: None,
            recording: None,
            in_flight: 0,
            model_state: None,
            flash: None,
            error: None,
            mic: macos::mic_status(),
            keys_down: HashSet::new(),
            last_request: None,
            agent_send,
            pending_send: false,
            listeners: Vec::new(),
            wispr: None,
            quit_when_idle: false,
            quit: false,
        }
    }

    fn init(&mut self) {
        let icons = Icons::load();
        match Tray::new(&self.config, &audio::input_device_names(), &icons.idle) {
            Ok(mut tray) => {
                tray.set_directories(&self.recent_dirs, self.bound.as_deref());
                self.tray = Some(tray);
            }
            Err(err) => eprintln!("could not create the status item: {err:#}"),
        }
        self.icons = Some(icons);
        match Hotkeys::new(&self.config.shortcuts, self.config.agent_delivery == AgentDelivery::Batch) {
            Ok(hotkeys) => {
                if let Some(err) = hotkeys.errors.first() {
                    self.error = Some((Instant::now(), err.clone()));
                }
                self.hotkeys = Some(hotkeys);
            }
            Err(err) => self.error = Some((Instant::now(), format!("Global shortcuts unavailable: {err}"))),
        }
        environment::track_browsers();
        // Ask for the microphone up front, while the user is at the terminal, not mid-bug.
        if self.mic == Mic::Undetermined && !cfg!(feature = "dev-hooks") {
            self.request_mic();
        }
        self.models.ensure(&self.config.model);
        self.watch_config();
        let proxy = self.proxy.clone();
        watch::listeners(paths::listeners_dir(), move |listeners| {
            proxy.send_event(UserEvent::Listeners(listeners)).is_ok()
        });
        self.sync_wispr();
        self.refresh_latest();
        self.refresh();
        self.publish();
    }

    fn handle(&mut self, event: UserEvent) {
        match event {
            UserEvent::HotKey(event) => self.on_hotkey(event),
            UserEvent::Menu(event) => self.on_menu(event),
            UserEvent::Work(outcome) => self.on_outcome(outcome),
            UserEvent::Model(id, state) => {
                if id == ModelSpec::find(&self.config.model).id {
                    self.model_state = Some(state);
                    self.refresh();
                    self.publish();
                }
            }
            UserEvent::Mic(granted) => {
                self.mic = if granted { Mic::Granted } else { Mic::Denied };
                self.refresh();
                self.publish();
            }
            UserEvent::ConfigFileChanged => self.reload_config(),
            UserEvent::Wispr(Ok(dictation)) => self.add_dictation(dictation),
            UserEvent::Wispr(Err(message)) => self.fail(message),
            UserEvent::Listeners(listeners) => {
                self.listeners = listeners;
                self.refresh();
            }
        }
    }

    fn handle_link(&mut self, url: &url::Url) {
        match Link::parse(url) {
            Some(Link::Bind { dir, new_note, title, request }) => {
                self.last_request = request;
                self.bind(dir, new_note.then_some(PendingNew { title }));
            }
            Some(Link::Quit) => self.request_quit(),
            Some(Link::Send { request }) => {
                self.last_request = request;
                self.send_to_agent();
                self.publish();
            }
            Some(Link::Simulate { wav, new_note, image, image_at, request }) => {
                self.last_request = request;
                #[cfg(feature = "dev-hooks")]
                if let Err(err) = self.simulate(&wav, new_note, image, image_at) {
                    self.fail(format!("simulate: {err:#}"));
                }
                #[cfg(not(feature = "dev-hooks"))]
                {
                    let _ = (wav, new_note, image, image_at);
                    self.fail("This build does not support `asides simulate`.".into());
                }
                self.publish();
            }
            None => {}
        }
    }

    fn on_hotkey(&mut self, event: GlobalHotKeyEvent) {
        let Some(action) = self.hotkeys.as_ref().and_then(|h| h.action(event.id)) else { return };
        let is_record = matches!(action, Action::Record | Action::RecordNewNote);
        match event.state {
            HotKeyState::Pressed => {
                // Ignore key repeat while a record shortcut is held down.
                if is_record && !self.keys_down.insert(event.id) {
                    return;
                }
                match action {
                    Action::Record | Action::RecordNewNote if self.recording.is_some() => self.stop_recording(),
                    Action::Record => self.start_recording(false, Some((action, Instant::now()))),
                    Action::RecordNewNote => self.start_recording(true, Some((action, Instant::now()))),
                    Action::AttachImage => self.attach_image(),
                    Action::Cancel => self.cancel_recording(),
                    Action::SendToAgent => self.send_to_agent(),
                }
            }
            HotKeyState::Released => {
                self.keys_down.remove(&event.id);
                let held = self
                    .recording
                    .as_ref()
                    .and_then(|r| r.pressed)
                    .is_some_and(|(a, at)| a == action && at.elapsed() >= TAP_MAX);
                if self.config.hold_to_talk && held {
                    self.stop_recording();
                }
            }
        }
    }

    fn on_menu(&mut self, event: MenuEvent) {
        let Some(action) = self.tray.as_ref().and_then(|t| t.action(&event.id)) else { return };
        let checked = self.tray.as_ref().is_some_and(|t| t.checked(&action));
        match action {
            MenuAction::Record if self.recording.is_some() => self.stop_recording(),
            MenuAction::Record => self.start_recording(false, None),
            MenuAction::RecordNewNote => self.start_recording(true, None),
            MenuAction::AttachImage => self.attach_image(),
            MenuAction::Cancel => self.cancel_recording(),
            MenuAction::SendToAgent => self.send_to_agent(),
            MenuAction::SwitchDirectory(dir) => self.bind_from_menu(&dir),
            MenuAction::ChooseDirectory => {
                if let Some(dir) = macos::choose_directory(self.mtm, self.bound.as_deref()) {
                    self.bind_from_menu(&dir);
                }
            }
            MenuAction::OpenNote => self.with_latest(macos::open),
            MenuAction::RevealNote => self.with_latest(macos::reveal),
            MenuAction::CopyNotePath => self.with_latest(|file| clipboard::copy_text(&file.to_string_lossy())),
            MenuAction::ToggleHoldToTalk => self.update_config(|c| c.hold_to_talk = checked),
            MenuAction::ToggleAutoAttach => self.update_config(|c| c.auto_attach_images = checked),
            MenuAction::ToggleSounds => self.update_config(|c| c.sounds = checked),
            MenuAction::ToggleKeepAudio => self.update_config(|c| c.keep_audio = checked),
            MenuAction::ToggleWisprFlow => {
                self.update_config(|c| c.wispr_flow = checked);
                self.sync_wispr();
            }
            MenuAction::AgentDelivery(mode) => {
                self.update_config(|c| c.agent_delivery = mode);
                self.apply_hotkeys();
                self.refresh();
            }
            MenuAction::Model(id) => {
                self.update_config(|c| c.model = id.clone());
                self.model_state = None;
                self.models.ensure(&id);
            }
            MenuAction::InputDevice(name) => self.update_config(|c| c.input_device = name.clone()),
            MenuAction::MicrophoneSettings => {
                if self.mic == Mic::Undetermined {
                    self.request_mic();
                } else {
                    macos::open_microphone_settings();
                }
            }
            MenuAction::EditSettings => macos::open_in_text_editor(&paths::config_path()),
            MenuAction::Quit => self.request_quit(),
        }
    }

    fn on_outcome(&mut self, outcome: Outcome) {
        self.in_flight = self.in_flight.saturating_sub(1);
        let now = Instant::now();
        match outcome {
            Outcome::Appended { note, number } => {
                self.error = None;
                self.flash = Some((now + FLASH, format!("#{number}"), format!("Added #{number} to {note}")));
            }
            Outcome::ImageAdded { note } => {
                self.flash = Some((now + FLASH, "📎".into(), format!("Image added to {note}")));
            }
            Outcome::NothingHeard => {
                self.flash = Some((now + FLASH, "–".into(), "Nothing heard; nothing was written".into()));
            }
            Outcome::Failed(message) => self.fail(message),
        }
        self.refresh_latest();
        self.flush_send();
        self.refresh();
        self.publish();
        if self.quit_when_idle && self.in_flight == 0 && self.recording.is_none() {
            self.quit = true;
        }
    }

    /// Points future recordings at `dir` (a recording in progress keeps its own directory).
    fn bind(&mut self, dir: PathBuf, pending_new: Option<PendingNew>) {
        self.recent_dirs.retain(|d| d.is_dir());
        app_state::remember_dir(&mut self.recent_dirs, &dir);
        self.bound = Some(dir);
        self.pending_new = pending_new;
        self.error = None;
        if let Some(tray) = &mut self.tray {
            tray.set_directories(&self.recent_dirs, self.bound.as_deref());
        }
        self.refresh_latest();
        self.refresh();
        self.publish();
    }

    /// Binding chosen in the menu: same rules as the CLI (repo root, local git exclude).
    fn bind_from_menu(&mut self, chosen: &Path) {
        let dir = match project::resolve_bind_dir(chosen) {
            Ok(dir) => dir,
            Err(err) => {
                self.recent_dirs.retain(|d| d != chosen);
                if let Some(tray) = &mut self.tray {
                    tray.set_directories(&self.recent_dirs, self.bound.as_deref());
                }
                return self.fail(format!("Cannot use {}: {err}", tilde(chosen)));
            }
        };
        if let Err(err) = project::ensure_git_exclude(&dir) {
            eprintln!("could not update .git/info/exclude: {err}");
        }
        let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.bind(dir.clone(), None);
        self.flash = Some((Instant::now() + FLASH, name, format!("Now writing to {}", tilde(&dir))));
        self.refresh();
    }

    fn start_recording(&mut self, force_new: bool, pressed: Option<(Action, Instant)>) {
        let Some(dir) = self.bound.clone() else {
            return self.fail("Not bound to a directory. Run `asides` in a repo first.".into());
        };
        if !dir.is_dir() {
            return self.fail(format!("{} no longer exists. Run `asides` again.", dir.display()));
        }
        match macos::mic_status() {
            Mic::Granted => self.mic = Mic::Granted,
            Mic::Undetermined => return self.request_mic(),
            Mic::Denied => {
                self.mic = Mic::Denied;
                return self.fail("Microphone access is denied. Preferences → Microphone Permission…".into());
            }
        }
        let recorder = match audio::Recorder::start(&self.config.input_device) {
            Ok(recorder) => recorder,
            Err(err) => return self.fail(format!("{err:#}")),
        };
        let pending = self.pending_new.take();
        let new_note = (force_new || pending.is_some()).then(|| pending.and_then(|p| p.title));
        let target_label = match (&new_note, &self.latest) {
            (Some(Some(title)), _) => format!("new note \"{title}\""),
            (Some(None), _) | (None, None) => "new note".into(),
            (None, Some(latest)) => latest.title.clone(),
        };
        self.recording = Some(Recording {
            recorder,
            started_at: Instant::now(),
            started: Local::now(),
            dir,
            new_note,
            target_label,
            images: Vec::new(),
            environment: environment::capture(self.mtm),
            pressed,
            clipboard: clipboard::change_count(),
        });
        if let Some(hotkeys) = &mut self.hotkeys {
            hotkeys.set_cancel_active(true);
        }
        self.error = None;
        self.flash = None;
        self.sound("Tink");
        self.refresh();
        self.publish();
    }

    fn stop_recording(&mut self) {
        let Some(recording) = self.recording.take() else { return };
        if let Some(hotkeys) = &mut self.hotkeys {
            hotkeys.set_cancel_active(false);
        }
        let (samples, sample_rate) = recording.recorder.finish();
        self.sound("Pop");
        let seconds = samples.len() as f32 / sample_rate.max(1) as f32;
        if seconds < MIN_RECORDING_SECS && recording.images.is_empty() {
            self.refresh();
            self.publish();
            return;
        }
        self.submit(Job::Recording(Box::new(RecordingJob {
            dir: recording.dir,
            samples,
            sample_rate,
            started: recording.started,
            new_note: recording.new_note,
            images: recording.images,
            environment: recording.environment,
            keep_audio: self.config.keep_audio,
            model: self.config.model.clone(),
            languages: self.config.languages.clone(),
        })));
    }

    fn cancel_recording(&mut self) {
        let Some(recording) = self.recording.take() else { return };
        if let Some(hotkeys) = &mut self.hotkeys {
            hotkeys.set_cancel_active(false);
        }
        drop(recording.recorder.finish());
        // A cancelled "new note" request stays pending for the next recording.
        if let Some(title) = recording.new_note {
            self.pending_new = Some(PendingNew { title });
        }
        self.sound("Funk");
        self.flash = Some((Instant::now() + FLASH, "✕".into(), "Recording cancelled".into()));
        self.refresh();
        self.publish();
    }

    fn attach_image(&mut self) {
        let Some(image) = clipboard::read_image() else {
            return self.fail("The clipboard has no image. Copy a screenshot first (⌃⇧⌘4).".into());
        };
        if let Some(recording) = &mut self.recording {
            recording.images.push(CapturedImage {
                offset: recording.started_at.elapsed().as_secs_f32(),
                at: Local::now(),
                bytes: image.bytes,
                extension: image.extension,
            });
            recording.clipboard = clipboard::change_count();
            self.sound("Tink");
            self.refresh();
            return;
        }
        let Some(dir) = self.bound.clone() else {
            return self.fail("Not bound to a directory. Run `asides` in a repo first.".into());
        };
        let new_note = self.pending_new.take().map(|p| p.title);
        self.submit(Job::Image(Box::new(ImageJob {
            dir,
            image: CapturedImage { offset: 0.0, at: Local::now(), bytes: image.bytes, extension: image.extension },
            new_note,
            environment: environment::capture(self.mtm),
            model: self.config.model.clone(),
        })));
    }

    /// A Wispr Flow dictation becomes an entry, like a recording.
    fn add_dictation(&mut self, dictation: Dictation) {
        if !wispr::app_matches(dictation.app.as_deref(), &self.config.wispr_flow_apps) {
            return;
        }
        let Some(dir) = self.bound.clone() else {
            return self
                .fail("A Wispr Flow dictation arrived, but no directory is bound. Run `asides` in a repo.".into());
        };
        if !dir.is_dir() {
            return self.fail(format!("{} no longer exists. Run `asides` again.", dir.display()));
        }
        let new_note = self.pending_new.take().map(|p| p.title);
        self.submit(Job::Dictation(Box::new(DictationJob {
            dir,
            text: dictation.text,
            at: dictation.at.with_timezone(&Local),
            source: "Wispr Flow",
            new_note,
            environment: environment::capture(self.mtm),
        })));
    }

    /// Hands what's been recorded so far to agents listening in batch mode. A recording in
    /// progress is stopped first, and the batch goes out once every recording is transcribed.
    fn send_to_agent(&mut self) {
        if self.bound.is_none() {
            return self.fail("Not bound to a directory. Run `asides` in a repo first.".into());
        }
        if self.recording.is_some() {
            self.stop_recording();
        }
        self.pending_send = true;
        self.flush_send();
    }

    fn flush_send(&mut self) {
        if !self.pending_send || self.in_flight > 0 {
            return;
        }
        self.pending_send = false;
        let Some(dir) = self.bound.clone() else { return };
        let latest = notes::latest_note(&dir);
        self.agent_send = Some(AgentSend {
            at_ms: now_ms(),
            entry: latest.as_ref().map_or(0, |n| n.next_entry_number() - 1),
            note: latest.map(|n| n.dir),
            dir,
        });
        let status = match self.listening() {
            Some(agents) => format!("Sent to {agents}"),
            None => "Sent, but no agent is listening right now".into(),
        };
        self.flash = Some((Instant::now() + FLASH, "Sent".into(), status));
        self.sound("Glass");
        self.refresh();
        self.publish();
    }

    /// The agents listening in the bound directory, e.g. "Claude Code" or "Claude Code and Codex".
    fn listening(&self) -> Option<String> {
        let now = now_ms();
        let mut names: Vec<String> = Vec::new();
        for listener in &self.listeners {
            let active = listener.waiting || now.saturating_sub(listener.updated_ms) < LISTENING_GRACE_MS;
            let name = agents::client_label(&listener.client);
            if active && Some(&listener.dir) == self.bound.as_ref() && !names.contains(&name) {
                names.push(name);
            }
        }
        match names.as_slice() {
            [] => None,
            [one] => Some(one.clone()),
            [rest @ .., last] => Some(format!("{} and {last}", rest.join(", "))),
        }
    }

    /// Starts or stops watching Wispr Flow's history to match the settings.
    fn sync_wispr(&mut self) {
        let source = self.wispr_source();
        if source.is_some() == self.wispr.is_some() {
            return;
        }
        if let Some(stop) = self.wispr.take() {
            stop.store(true, Ordering::Relaxed);
        }
        if let Some(db) = source {
            let proxy = self.proxy.clone();
            self.wispr = Some(watch::wispr_flow(db, move |found| proxy.send_event(UserEvent::Wispr(found)).is_ok()));
        }
    }

    fn wispr_source(&self) -> Option<PathBuf> {
        // The end-to-end test stands in for Wispr Flow with a database of its own.
        #[cfg(feature = "dev-hooks")]
        {
            let test = paths::support_dir().join("test-wispr-flow.sqlite");
            if test.is_file() {
                return Some(test);
            }
        }
        self.config.wispr_flow.then(wispr::default_db)
    }

    fn submit(&mut self, job: Job) {
        self.in_flight += 1;
        if self.jobs.send(job).is_err() {
            self.in_flight -= 1;
            self.fail("The transcription worker stopped. Please restart Asides.".into());
        }
        self.refresh();
        self.publish();
    }

    fn tick(&mut self) {
        let auto_attach = self.config.auto_attach_images;
        if let Some(recording) = &mut self.recording {
            let count = clipboard::change_count();
            if auto_attach && count != recording.clipboard {
                recording.clipboard = count;
                if clipboard::has_image()
                    && let Some(image) = clipboard::read_image()
                {
                    recording.images.push(CapturedImage {
                        offset: recording.started_at.elapsed().as_secs_f32(),
                        at: Local::now(),
                        bytes: image.bytes,
                        extension: image.extension,
                    });
                    self.sound("Tink");
                }
            }
        }
        if self.flash.as_ref().is_some_and(|(until, ..)| Instant::now() >= *until) {
            self.flash = None;
        }
        self.refresh();
    }

    fn next_wakeup(&self) -> Option<Instant> {
        let now = Instant::now();
        let error_icon = self.error.as_ref().is_some_and(|(at, _)| now < *at + ERROR_ICON);
        (self.recording.is_some() || self.flash.is_some() || error_icon).then_some(now + TICK)
    }

    fn refresh(&mut self) {
        let Some(icons) = &self.icons else { return };
        let now = Instant::now();
        let dir_name = self.bound.as_ref().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().into_owned());
        let downloading = match &self.model_state {
            Some(ModelState::Downloading { progress }) => Some((progress * 100.0).floor() as u32),
            _ => None,
        };
        let model_error = match &self.model_state {
            Some(ModelState::Failed(message)) => Some(message.clone()),
            _ => None,
        };
        let error_text = self.error.as_ref().map(|(_, m)| m.clone()).or(model_error.clone());
        let error_icon = self.error.as_ref().is_some_and(|(at, _)| now < *at + ERROR_ICON) || model_error.is_some();

        let (icon_key, icon, title, status) = if let Some(recording) = &self.recording {
            let secs = recording.started_at.elapsed().as_secs();
            let clock = format!("{}:{:02}", secs / 60, secs % 60);
            let images = match recording.images.len() {
                0 => String::new(),
                n => format!(" · 📎{n}"),
            };
            (
                "recording",
                &icons.recording,
                Some(format!("{clock}{images}")),
                format!("● Recording {clock} → {}", recording.target_label),
            )
        } else if self.in_flight > 0 {
            let status = match downloading {
                Some(p) => format!("Waiting for the speech model download… {p}%"),
                None if self.in_flight > 1 => format!("Transcribing… ({} recordings queued)", self.in_flight),
                None => "Transcribing…".into(),
            };
            ("busy", &icons.busy, None, status)
        } else if let Some((_, title, status)) = &self.flash {
            ("done", &icons.done, Some(title.clone()), status.clone())
        } else if error_icon {
            ("error", &icons.error, None, format!("⚠ {}", error_text.clone().unwrap_or_default()))
        } else if let Some(p) = downloading {
            ("download", &icons.download, Some(format!("{p}%")), format!("Downloading speech model… {p}%"))
        } else if let Some(dir) = &dir_name {
            let status = match (&error_text, self.listening()) {
                (Some(message), _) => format!("⚠ {message}"),
                (None, Some(agents)) => format!("Ready · {dir} · {agents} listening"),
                (None, None) => format!("Ready · {dir}"),
            };
            ("idle", &icons.idle, None, status)
        } else {
            ("unbound", &icons.unbound, None, "Not bound · run `asides` in a repo".into())
        };

        let note_info = match (&dir_name, &self.pending_new, &self.latest) {
            (None, ..) => "No directory".into(),
            (Some(dir), Some(pending), _) => match &pending.title {
                Some(title) => format!("{dir} › next recording starts \"{title}\""),
                None => format!("{dir} › next recording starts a new note"),
            },
            (Some(dir), None, Some(latest)) => format!("{dir} › {} · {}", latest.title, entries(latest.entries)),
            (Some(dir), None, None) => format!("{dir} · no notes yet"),
        };
        let view = View {
            icon_key,
            icon,
            title,
            status,
            recording: self.recording.is_some(),
            bound: self.bound.as_deref(),
            note_info,
            has_note: self.latest.is_some(),
            send_label: match self.listening() {
                Some(agents) if !agents.contains(" and ") => format!("Send to {agents}"),
                _ => "Send to Agent".into(),
            },
            send_enabled: self.config.agent_delivery == AgentDelivery::Batch && self.bound.is_some(),
        };
        if let Some(tray) = &mut self.tray {
            tray.show(&view);
        }
    }

    fn refresh_latest(&mut self) {
        self.latest = self.bound.as_deref().and_then(notes::latest_note).map(|note| LatestNote {
            entries: note.entry_count(),
            file: note.file(),
            title: note.title,
        });
    }

    /// Publishes the app's state for the CLI (`asides status`, bind confirmation).
    fn publish(&self) {
        let status = if self.recording.is_some() {
            "recording"
        } else if self.in_flight > 0 {
            "transcribing"
        } else if matches!(self.model_state, Some(ModelState::Downloading { .. })) {
            "downloading"
        } else if self.error.is_some() || matches!(self.model_state, Some(ModelState::Failed(_))) {
            "error"
        } else {
            "idle"
        };
        let detail = match (&self.error, &self.model_state) {
            (Some((_, message)), _) => Some(message.clone()),
            (None, Some(ModelState::Failed(message))) => Some(message.clone()),
            (None, Some(ModelState::Downloading { progress })) => {
                Some(format!("Downloading the speech model: {:.0}%", progress * 100.0))
            }
            _ => None,
        };
        let state = AppState {
            pid: if self.quit { 0 } else { std::process::id() },
            bound_dir: self.bound.clone(),
            pending_new: self.pending_new.clone(),
            status: status.into(),
            detail,
            microphone: self.mic.as_str().into(),
            last_request: self.last_request.clone(),
            recent_dirs: self.recent_dirs.clone(),
            agent_send: self.agent_send.clone(),
            updated_ms: now_ms(),
        };
        if let Err(err) = state.write(&paths::state_path()) {
            eprintln!("could not write state.json: {err}");
        }
    }

    fn fail(&mut self, message: String) {
        self.error = Some((Instant::now(), message));
        self.sound("Basso");
        self.refresh();
        self.publish();
    }

    fn sound(&self, name: &str) {
        if self.config.sounds {
            macos::play(name);
        }
    }

    fn request_mic(&mut self) {
        let proxy = self.proxy.clone();
        macos::request_mic(move |granted| {
            let _ = proxy.send_event(UserEvent::Mic(granted));
        });
    }

    fn request_quit(&mut self) {
        if self.recording.is_some() {
            self.stop_recording();
        }
        if self.in_flight == 0 {
            self.quit = true;
            self.publish();
        } else {
            // Finish the queued transcriptions first.
            self.quit_when_idle = true;
        }
    }

    fn with_latest(&mut self, f: impl FnOnce(&std::path::Path)) {
        self.refresh_latest();
        match &self.latest {
            Some(latest) => f(&latest.file),
            None => self.fail("There is no note yet.".into()),
        }
    }

    fn update_config(&mut self, change: impl FnOnce(&mut Config)) {
        change(&mut self.config);
        if let Err(err) = self.config.save(&paths::config_path()) {
            self.fail(format!("Could not save settings: {err}"));
        }
        if let Some(tray) = &self.tray {
            tray.sync_config(&self.config);
        }
    }

    fn reload_config(&mut self) {
        let config = match Config::load(&paths::config_path()) {
            Ok(config) => config,
            Err(err) => return self.fail(format!("config.toml: {err}")),
        };
        if config == self.config {
            return;
        }
        let hotkeys_changed =
            config.shortcuts != self.config.shortcuts || config.agent_delivery != self.config.agent_delivery;
        if config.model != self.config.model {
            self.model_state = None;
            self.models.ensure(&config.model);
        }
        self.config = config;
        if hotkeys_changed {
            self.apply_hotkeys();
        }
        self.sync_wispr();
        if let Some(tray) = &self.tray {
            tray.sync_config(&self.config);
        }
        self.refresh();
    }

    fn apply_hotkeys(&mut self) {
        let Some(hotkeys) = &mut self.hotkeys else { return };
        hotkeys.apply(&self.config.shortcuts, self.config.agent_delivery == AgentDelivery::Batch);
        if self.recording.is_some() {
            hotkeys.set_cancel_active(true);
        }
        if let Some(err) = hotkeys.errors.first() {
            self.error = Some((Instant::now(), err.clone()));
        }
    }

    fn watch_config(&self) {
        let proxy = self.proxy.clone();
        let path = paths::config_path();
        std::thread::spawn(move || {
            let modified = || std::fs::metadata(&path).and_then(|m| m.modified()).ok();
            let mut last = modified();
            loop {
                std::thread::sleep(Duration::from_secs(2));
                let now = modified();
                if now != last {
                    last = now;
                    if proxy.send_event(UserEvent::ConfigFileChanged).is_err() {
                        return;
                    }
                }
            }
        });
    }

    #[cfg(feature = "dev-hooks")]
    fn simulate(
        &mut self,
        wav: &std::path::Path,
        new_note: bool,
        image: Option<PathBuf>,
        image_at: f32,
    ) -> anyhow::Result<()> {
        use anyhow::Context;
        let dir = self.bound.clone().context("not bound")?;
        let mut reader = hound::WavReader::open(wav)?;
        let spec = reader.spec();
        let interleaved: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Int => reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / (1i64 << (spec.bits_per_sample - 1)) as f32))
                .collect::<Result<_, _>>()?,
            hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        };
        let channels = spec.channels.max(1) as usize;
        let samples = interleaved.chunks(channels).map(|f| f.iter().sum::<f32>() / channels as f32).collect();
        let images = match image {
            Some(path) => vec![CapturedImage {
                offset: image_at,
                at: Local::now(),
                bytes: std::fs::read(&path)?,
                extension: path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "png".into()),
            }],
            None => Vec::new(),
        };
        let pending = self.pending_new.take();
        let new_note = (new_note || pending.is_some()).then(|| pending.and_then(|p| p.title));
        self.submit(Job::Recording(Box::new(RecordingJob {
            dir,
            samples,
            sample_rate: spec.sample_rate,
            started: Local::now(),
            new_note,
            images,
            environment: environment::capture(self.mtm),
            keep_audio: self.config.keep_audio,
            model: self.config.model.clone(),
            languages: self.config.languages.clone(),
        })));
        Ok(())
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn entries(n: u32) -> String {
    if n == 1 { "1 entry".into() } else { format!("{n} entries") }
}
