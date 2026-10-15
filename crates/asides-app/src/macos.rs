//! Small AppKit/AVFoundation helpers: sounds, microphone permission, Finder.

use std::path::{Path, PathBuf};
use std::process::Command;

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::runtime::Bool;
use objc2_app_kit::{NSApplication, NSModalResponse, NSOpenPanel, NSSound};
use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
use objc2_foundation::{NSString, NSURL};

const MODAL_RESPONSE_OK: NSModalResponse = 1;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mic {
    Granted,
    Denied,
    Undetermined,
}

impl Mic {
    pub fn as_str(self) -> &'static str {
        match self {
            Mic::Granted => "granted",
            Mic::Denied => "denied",
            Mic::Undetermined => "undetermined",
        }
    }
}

pub fn mic_status() -> Mic {
    let Some(audio) = (unsafe { AVMediaTypeAudio }) else { return Mic::Undetermined };
    match unsafe { AVCaptureDevice::authorizationStatusForMediaType(audio) } {
        AVAuthorizationStatus::Authorized => Mic::Granted,
        AVAuthorizationStatus::NotDetermined => Mic::Undetermined,
        _ => Mic::Denied,
    }
}

/// Shows the system microphone prompt; `done` runs on an arbitrary thread with the answer.
pub fn request_mic(done: impl Fn(bool) + 'static) {
    let Some(audio) = (unsafe { AVMediaTypeAudio }) else { return };
    let block = RcBlock::new(move |granted: Bool| done(granted.as_bool()));
    unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(audio, &block) };
}

/// Shows the system folder picker (starting at `start`) and returns the chosen directory.
pub fn choose_directory(mtm: MainThreadMarker, start: Option<&Path>) -> Option<PathBuf> {
    let app = NSApplication::sharedApplication(mtm);
    // A menu-bar app must come to the front for its panel to get keyboard focus.
    app.activate();
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseDirectories(true);
    panel.setCanChooseFiles(false);
    panel.setAllowsMultipleSelection(false);
    panel.setCanCreateDirectories(true);
    panel.setPrompt(Some(&NSString::from_str("Use This Directory")));
    panel.setMessage(Some(&NSString::from_str("Recordings will be appended to notes in this repo or directory.")));
    if let Some(start) = start {
        panel.setDirectoryURL(Some(&NSURL::fileURLWithPath(&NSString::from_str(&start.to_string_lossy()))));
    }
    let chosen = (panel.runModal() == MODAL_RESPONSE_OK)
        .then(|| panel.URL()?.path())
        .flatten()
        .map(|path| PathBuf::from(path.to_string()));
    // Hand focus back to the app you were in (typically the browser).
    app.hide(None);
    chosen
}

pub fn play(sound: &str) {
    if let Some(sound) = NSSound::soundNamed(&NSString::from_str(sound)) {
        sound.stop();
        sound.play();
    }
}

pub fn open(path: &Path) {
    let _ = Command::new("open").arg(path).spawn();
}

pub fn open_in_text_editor(path: &Path) {
    let _ = Command::new("open").arg("-t").arg(path).spawn();
}

pub fn reveal(path: &Path) {
    let _ = Command::new("open").arg("-R").arg(path).spawn();
}

pub fn open_microphone_settings() {
    let _ =
        Command::new("open").arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone").spawn();
}
