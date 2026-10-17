//! Global shortcuts (Carbon hot keys via `global-hotkey`; no Accessibility permission needed).
//! They work while Asides is open too, over the live screen as well as frozen.

use std::str::FromStr;

use asides_core::config::Shortcuts;
use global_hotkey::GlobalHotKeyManager;
use global_hotkey::hotkey::HotKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Opens Asides; while it's open, a tap adds it all to the aside and a hold talks.
    Asides,
    /// The same, into a new note.
    NewNote,
    AttachImage,
    SendToAgent,
    /// Freezes the screen, or goes back to live.
    Freeze,
    /// Switch the target to the next agent session.
    NextTarget,
}

pub struct Hotkeys {
    manager: GlobalHotKeyManager,
    bindings: Vec<(HotKey, Action)>,
    /// Shortcuts that could not be parsed or registered (e.g. taken by another app).
    pub errors: Vec<String>,
}

impl Hotkeys {
    pub fn new(shortcuts: &Shortcuts, send: bool, sessions: bool) -> anyhow::Result<Hotkeys> {
        let mut hotkeys = Hotkeys { manager: GlobalHotKeyManager::new()?, bindings: Vec::new(), errors: Vec::new() };
        hotkeys.apply(shortcuts, send, sessions);
        Ok(hotkeys)
    }

    /// (Re)registers all shortcuts: Send to Agent only with `send` (batch delivery), and
    /// switching sessions only while there are `sessions`, so those shortcuts stay free for other
    /// apps otherwise.
    pub fn apply(&mut self, shortcuts: &Shortcuts, send: bool, sessions: bool) {
        for (hotkey, _) in self.bindings.drain(..) {
            let _ = self.manager.unregister(hotkey);
        }
        self.errors.clear();
        let send = send.then_some((&shortcuts.send_to_agent, Action::SendToAgent));
        let next = sessions.then_some((&shortcuts.next_target, Action::NextTarget));
        for (text, action) in [
            (&shortcuts.asides, Action::Asides),
            (&shortcuts.new_note, Action::NewNote),
            (&shortcuts.attach_image, Action::AttachImage),
            (&shortcuts.freeze, Action::Freeze),
        ]
        .into_iter()
        .chain(send)
        .chain(next)
        .filter(|(text, _)| !text.trim().is_empty())
        {
            match HotKey::from_str(text) {
                Ok(hotkey) => match self.manager.register(hotkey) {
                    Ok(()) => self.bindings.push((hotkey, action)),
                    Err(err) => self.errors.push(format!("Shortcut {text} is unavailable: {err}")),
                },
                Err(err) => self.errors.push(format!("Shortcut {text} is invalid: {err}")),
            }
        }
    }

    pub fn action(&self, id: u32) -> Option<Action> {
        self.bindings.iter().find(|(hotkey, _)| hotkey.id() == id).map(|(_, action)| *action)
    }
}
