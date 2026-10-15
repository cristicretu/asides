//! Facts about the machine recorded in a new note's front matter: macOS, browser, display.

use std::ptr::NonNull;
use std::sync::Mutex;

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSRunningApplication, NSScreen, NSWorkspace, NSWorkspaceApplicationKey,
    NSWorkspaceDidActivateApplicationNotification,
};
use objc2_foundation::{NSBundle, NSNotification, NSOperationQueue, NSProcessInfo, NSString};

const BROWSERS: &[&str] = &[
    "com.apple.Safari",
    "com.apple.SafariTechnologyPreview",
    "org.mozilla.firefox",
    "org.mozilla.firefoxdeveloperedition",
    "org.mozilla.nightly",
    "com.google.Chrome",
    "com.google.Chrome.canary",
    "org.chromium.Chromium",
    "company.thebrowser.Browser",
    "company.thebrowser.dia",
    "com.brave.Browser",
    "com.microsoft.edgemac",
    "app.zen-browser.zen",
    "com.kagi.kagimacOS",
    "com.vivaldi.Vivaldi",
    "com.operasoftware.Opera",
];

/// "Firefox 143.0" of the browser most recently brought to the front.
static LAST_BROWSER: Mutex<Option<String>> = Mutex::new(None);

#[derive(Debug, Clone, Default)]
pub struct Environment {
    pub macos: Option<String>,
    pub browser: Option<String>,
    pub display: Option<String>,
}

/// Remembers the last active browser, so a note started from the terminal still names it.
pub fn track_browsers() {
    if let Some(app) = NSWorkspace::sharedWorkspace().frontmostApplication() {
        remember_if_browser(&app);
    }
    let block = RcBlock::new(|notification: NonNull<NSNotification>| {
        let notification = unsafe { notification.as_ref() };
        let Some(info) = notification.userInfo() else { return };
        let key: &AnyObject = unsafe { NSWorkspaceApplicationKey };
        if let Some(app) = info.objectForKey(key).and_then(|o| o.downcast::<NSRunningApplication>().ok()) {
            remember_if_browser(&app);
        }
    });
    let center = NSWorkspace::sharedWorkspace().notificationCenter();
    let observer = unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(NSWorkspaceDidActivateApplicationNotification),
            None,
            Some(&NSOperationQueue::mainQueue()),
            &block,
        )
    };
    // The observer lives for the whole process.
    std::mem::forget(observer);
}

pub fn capture(mtm: MainThreadMarker) -> Environment {
    let frontmost = NSWorkspace::sharedWorkspace().frontmostApplication();
    let browser = frontmost
        .as_deref()
        .filter(|app| is_browser(app))
        .and_then(describe)
        .or_else(|| LAST_BROWSER.lock().ok()?.clone());
    Environment { macos: Some(macos_version()), browser, display: display(mtm) }
}

fn remember_if_browser(app: &NSRunningApplication) {
    if is_browser(app)
        && let Some(description) = describe(app)
        && let Ok(mut last) = LAST_BROWSER.lock()
    {
        *last = Some(description);
    }
}

fn is_browser(app: &NSRunningApplication) -> bool {
    app.bundleIdentifier().is_some_and(|id| BROWSERS.contains(&id.to_string().as_str()))
}

fn describe(app: &NSRunningApplication) -> Option<String> {
    let name = app.localizedName()?.to_string();
    let version = app
        .bundleURL()
        .and_then(|url| NSBundle::bundleWithURL(&url))
        .and_then(|bundle| bundle.objectForInfoDictionaryKey(&NSString::from_str("CFBundleShortVersionString")))
        .and_then(|v| v.downcast::<NSString>().ok())
        .map(|v: Retained<NSString>| v.to_string());
    Some(match version {
        Some(version) => format!("{name} {version}"),
        None => name,
    })
}

fn macos_version() -> String {
    let v = NSProcessInfo::processInfo().operatingSystemVersion();
    if v.patchVersion == 0 {
        format!("{}.{}", v.majorVersion, v.minorVersion)
    } else {
        format!("{}.{}.{}", v.majorVersion, v.minorVersion, v.patchVersion)
    }
}

fn display(mtm: MainThreadMarker) -> Option<String> {
    let screen = NSScreen::mainScreen(mtm)?;
    let size = screen.frame().size;
    let scale = screen.backingScaleFactor();
    Some(format!("{}x{} @{}x", (size.width * scale).round(), (size.height * scale).round(), scale.round()))
}
