//! Global shortcuts (Carbon hot keys via `global-hotkey`; no Accessibility permission needed).

use std::str::FromStr;

use asides_core::config::Shortcuts;
use global_hotkey::GlobalHotKeyManager;
use global_hotkey::hotkey::HotKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Record,
    RecordNewNote,
    AttachImage,
    Cancel,
}

pub struct Hotkeys {
    manager: GlobalHotKeyManager,
    bindings: Vec<(HotKey, Action)>,
    cancel: Option<HotKey>,
    cancel_active: bool,
    /// Shortcuts that could not be parsed or registered (e.g. taken by another app).
    pub errors: Vec<String>,
}

impl Hotkeys {
    pub fn new(shortcuts: &Shortcuts) -> anyhow::Result<Hotkeys> {
        let mut hotkeys = Hotkeys {
            manager: GlobalHotKeyManager::new()?,
            bindings: Vec::new(),
            cancel: None,
            cancel_active: false,
            errors: Vec::new(),
        };
        hotkeys.apply(shortcuts);
        Ok(hotkeys)
    }

    /// (Re)registers all shortcuts. Cancel is only registered while recording, so Esc keeps
    /// working normally in other apps the rest of the time.
    pub fn apply(&mut self, shortcuts: &Shortcuts) {
        for (hotkey, _) in self.bindings.drain(..) {
            let _ = self.manager.unregister(hotkey);
        }
        self.set_cancel_active(false);
        self.errors.clear();
        for (text, action) in [
            (&shortcuts.record, Action::Record),
            (&shortcuts.record_new_note, Action::RecordNewNote),
            (&shortcuts.attach_image, Action::AttachImage),
        ] {
            match HotKey::from_str(text) {
                Ok(hotkey) => match self.manager.register(hotkey) {
                    Ok(()) => self.bindings.push((hotkey, action)),
                    Err(err) => self.errors.push(format!("Shortcut {text} is unavailable: {err}")),
                },
                Err(err) => self.errors.push(format!("Shortcut {text} is invalid: {err}")),
            }
        }
        self.cancel = match HotKey::from_str(&shortcuts.cancel) {
            Ok(hotkey) => Some(hotkey),
            Err(err) => {
                self.errors.push(format!("Shortcut {} is invalid: {err}", shortcuts.cancel));
                None
            }
        };
    }

    pub fn action(&self, id: u32) -> Option<Action> {
        if self.cancel_active && self.cancel.is_some_and(|c| c.id() == id) {
            return Some(Action::Cancel);
        }
        self.bindings.iter().find(|(hotkey, _)| hotkey.id() == id).map(|(_, action)| *action)
    }

    pub fn set_cancel_active(&mut self, active: bool) {
        let Some(cancel) = self.cancel else { return };
        if active == self.cancel_active {
            return;
        }
        let result = if active { self.manager.register(cancel) } else { self.manager.unregister(cancel) };
        self.cancel_active = active && result.is_ok();
    }
}
