//! `~/Library/Application Support/Asides/config.toml`

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

const HEADER: &str = r#"# Asides settings. Changes are picked up automatically.
# Shortcut syntax: modifiers (ctrl, alt/option, shift, cmd) + a key (Space, Escape, KeyN, Digit1, F5, …),
# joined with '+'. Avoid option-only combinations: macOS refuses them.
# languages: Whisper codes to choose between, e.g. ["en", "ro"]; [] detects any language.

"#;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Holding the record shortcut records until release; a short tap toggles.
    pub hold_to_talk: bool,
    /// Images copied to the clipboard while recording are attached automatically.
    pub auto_attach_images: bool,
    pub sounds: bool,
    /// Keep an .m4a of each recording next to the note.
    pub keep_audio: bool,
    /// Speech model id, see `models::MODELS`.
    pub model: String,
    /// Input device name; empty means the system default.
    pub input_device: String,
    /// Whisper language codes a recording can be in (e.g. `["en", "ro"]`): each recording is
    /// transcribed in the most likely of these. Empty means any language Whisper knows.
    pub languages: Vec<String>,
    pub shortcuts: Shortcuts,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Shortcuts {
    pub record: String,
    pub record_new_note: String,
    pub attach_image: String,
    pub cancel: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hold_to_talk: true,
            auto_attach_images: true,
            sounds: true,
            keep_audio: false,
            model: crate::models::MODELS[0].id.into(),
            input_device: String::new(),
            languages: Vec::new(),
            shortcuts: Shortcuts::default(),
        }
    }
}

impl Default for Shortcuts {
    fn default() -> Self {
        Self {
            record: "ctrl+alt+Space".into(),
            record_new_note: "ctrl+alt+KeyN".into(),
            attach_image: "ctrl+alt+KeyV".into(),
            cancel: "Escape".into(),
        }
    }
}

impl Config {
    /// Reads the config, falling back to defaults for a missing file or missing keys.
    pub fn load(path: &Path) -> io::Result<Config> {
        match fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e),
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let body = toml::to_string_pretty(self).map_err(|e| io::Error::other(e.to_string()))?;
        fs::write(path, format!("{HEADER}{body}"))
    }
}

/// `ctrl+alt+Space` → `⌃⌥Space`, `cmd+shift+KeyN` → `⇧⌘N`.
pub fn pretty_shortcut(shortcut: &str) -> String {
    let mut mods = String::new();
    let mut key = String::new();
    let tokens: Vec<String> = shortcut.split('+').map(|t| t.trim().to_ascii_lowercase()).collect();
    for symbol in ["⌃", "⌥", "⇧", "⌘"] {
        let present = tokens.iter().any(|t| match symbol {
            "⌃" => t == "ctrl" || t == "control",
            "⌥" => t == "alt" || t == "option",
            "⇧" => t == "shift",
            _ => t == "cmd" || t == "command" || t == "super",
        });
        if present {
            mods.push_str(symbol);
        }
    }
    if let Some(last) = shortcut.split('+').map(str::trim).next_back() {
        let lower = last.to_ascii_lowercase();
        if !["ctrl", "control", "alt", "option", "shift", "cmd", "command", "super"].contains(&lower.as_str()) {
            key = match lower.as_str() {
                "escape" | "esc" => "Esc".into(),
                "space" => "Space".into(),
                _ => last.strip_prefix("Key").or_else(|| last.strip_prefix("Digit")).unwrap_or(last).to_uppercase(),
            };
        }
    }
    format!("{mods}{key}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_prints_shortcuts() {
        assert_eq!(pretty_shortcut("ctrl+alt+Space"), "⌃⌥Space");
        assert_eq!(pretty_shortcut("cmd+shift+KeyN"), "⇧⌘N");
        assert_eq!(pretty_shortcut("Escape"), "Esc");
        assert_eq!(pretty_shortcut("ctrl+alt+Digit1"), "⌃⌥1");
        assert_eq!(pretty_shortcut("option+F5"), "⌥F5");
    }

    #[test]
    fn round_trips_and_fills_missing_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        assert_eq!(Config::load(&path).unwrap(), Config::default());
        let config = Config {
            keep_audio: true,
            shortcuts: Shortcuts { record: "cmd+shift+KeyR".into(), ..Default::default() },
            ..Default::default()
        };
        config.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap(), config);

        fs::write(&path, "sounds = false\n[shortcuts]\ncancel = \"F12\"\n").unwrap();
        let partial = Config::load(&path).unwrap();
        assert!(!partial.sounds && partial.hold_to_talk);
        assert_eq!(partial.shortcuts.cancel, "F12");
        assert_eq!(partial.shortcuts.record, "ctrl+alt+Space");
    }
}
