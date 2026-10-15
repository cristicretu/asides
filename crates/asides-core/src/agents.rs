//! Handing entries to coding agents through `asides mcp`: what a listening agent has already
//! seen, what's new since, and the listener records the app shows in its menu.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::notes::{self, Entry, Note};

/// How far an agent has read: entry `entry` of `note` (a note folder), of which the first `len`
/// bytes of the body. `note: None` means before the first note.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cursor {
    pub note: Option<PathBuf>,
    pub entry: u32,
    pub len: usize,
}

/// Something an agent hasn't seen yet.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// A new entry.
    Entry { note: Note, entry: Entry },
    /// Text added to an entry the agent already has, such as an image attached after it.
    Addition { note: Note, number: u32, text: String },
}

impl Cursor {
    /// The end of `base`'s notes as they are now: only entries written later are new.
    pub fn at_end(base: &Path) -> Cursor {
        let Some(note) = notes::latest_note(base) else { return Cursor::default() };
        let last = fs::read_to_string(note.file()).ok().and_then(|c| notes::parse_entries(&c).pop());
        Cursor {
            entry: last.as_ref().map_or(0, |e| e.number),
            len: last.map_or(0, |e| e.body.len()),
            note: Some(note.dir),
        }
    }

    /// Moves past `item`.
    pub fn advance(&mut self, item: &Item) {
        match item {
            Item::Entry { note, entry } => {
                *self = Cursor { note: Some(note.dir.clone()), entry: entry.number, len: entry.body.len() };
            }
            Item::Addition { text, .. } => self.len += text.len(),
        }
    }
}

/// What `base`'s notes hold beyond `cursor`, oldest first. With `until` (a note folder and an
/// entry number), entries after that point are left for later.
pub fn collect(base: &Path, cursor: &Cursor, until: Option<(&Path, u32)>) -> Vec<Item> {
    let all = notes::list_notes(base);
    let start = match &cursor.note {
        None => 0,
        Some(dir) => match all.iter().position(|n| &n.dir == dir) {
            Some(i) => i,
            // The note is gone (deleted or renamed): carry on with the notes named after it.
            None => all.iter().position(|n| n.dir.file_name() > dir.file_name()).unwrap_or(all.len()),
        },
    };
    let stop = until.and_then(|(dir, _)| all.iter().position(|n| n.dir == dir));
    let mut items = Vec::new();
    for (i, note) in all.iter().enumerate().skip(start) {
        if stop.is_some_and(|stop| i > stop) {
            break;
        }
        let Ok(content) = fs::read_to_string(note.file()) else { continue };
        let same = cursor.note.as_ref() == Some(&note.dir);
        let last = until.filter(|_| stop == Some(i)).map(|(_, entry)| entry);
        for entry in notes::parse_entries(&content) {
            if same && entry.number < cursor.entry {
                continue;
            }
            if same && entry.number == cursor.entry {
                let added = entry.body.get(cursor.len..).map(str::trim).unwrap_or_default();
                if entry.body.len() > cursor.len && !added.is_empty() {
                    let text = entry.body[cursor.len..].to_string();
                    items.push(Item::Addition { note: note.clone(), number: entry.number, text });
                }
                continue;
            }
            if last.is_some_and(|last| entry.number > last) {
                break;
            }
            items.push(Item::Entry { note: note.clone(), entry });
        }
    }
    items
}

/// Entry text as an agent reads it: links to the note's images and audio made absolute (in angle
/// brackets, since note folders have spaces in their names), so the agent can open them.
pub fn absolute_links(note_dir: &Path, text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("](") {
        out.push_str(&rest[..i + 2]);
        rest = &rest[i + 2..];
        let Some(end) = rest.find(')') else { break };
        let target = &rest[..end];
        if is_note_relative(target) {
            out.push_str(&format!("<{}>", note_dir.join(target).display()));
            rest = &rest[end..];
        }
    }
    out.push_str(rest);
    out
}

/// The images an entry shows, as absolute paths, in order.
pub fn image_paths(note_dir: &Path, text: &str) -> Vec<PathBuf> {
    text.match_indices("![")
        .filter_map(|(i, _)| {
            let after = &text[i..];
            let start = after.find("](")? + 2;
            let end = start + after[start..].find(')')?;
            let target = &after[start..end];
            is_note_relative(target).then(|| note_dir.join(target))
        })
        .collect()
}

fn is_note_relative(target: &str) -> bool {
    (target.starts_with("images/") || target.starts_with("audio/")) && !target.contains("..")
}

/// An agent session that listens through `asides mcp`, as the app shows it in its menu.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Listener {
    pub pid: u32,
    /// The agent's name, e.g. "Claude Code".
    pub client: String,
    /// The bound directory it listens in.
    pub dir: PathBuf,
    /// Waiting for asides right now; otherwise working on the last ones it got.
    pub waiting: bool,
    pub updated_ms: u64,
}

impl Listener {
    pub fn file(listeners: &Path, pid: u32) -> PathBuf {
        listeners.join(format!("{pid}.json"))
    }

    /// Atomic write (temp file + rename), like `state.json`.
    pub fn write(&self, listeners: &Path) -> io::Result<()> {
        fs::create_dir_all(listeners)?;
        let path = Listener::file(listeners, self.pid);
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec(self).map_err(io::Error::other)?)?;
        fs::rename(tmp, path)
    }

    /// Every listener record in `listeners`, including those of processes that have since died.
    pub fn read_all(listeners: &Path) -> Vec<Listener> {
        let Ok(files) = fs::read_dir(listeners) else { return Vec::new() };
        let mut all: Vec<Listener> = files
            .flatten()
            .filter(|f| f.path().extension().is_some_and(|e| e == "json"))
            .filter_map(|f| serde_json::from_str(&fs::read_to_string(f.path()).ok()?).ok())
            .collect();
        all.sort_by_key(|l| l.pid);
        all
    }
}

/// A friendlier name for an MCP client: `claude-code` → "Claude Code".
pub fn client_label(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if lower.contains("claude") {
        "Claude Code".into()
    } else if lower.contains("codex") {
        "Codex".into()
    } else if lower.contains("opencode") {
        "OpenCode".into()
    } else if name.trim().is_empty() {
        "Agent".into()
    } else {
        name.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, TimeZone};

    fn note(base: &Path, minute: u32, title: &str) -> Note {
        let at = Local.with_ymd_and_hms(2026, 10, 15, 14, minute, 0).unwrap();
        notes::create_note(base, at, title, "---\ntitle: x\n---").unwrap()
    }

    fn numbers(items: &[Item]) -> Vec<String> {
        items
            .iter()
            .map(|item| match item {
                Item::Entry { note, entry } => format!("{} #{}", note.title, entry.number),
                Item::Addition { note, number, text } => format!("{} #{number} +{}", note.title, text.trim()),
            })
            .collect()
    }

    #[test]
    fn collects_new_entries_and_additions() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(Cursor::at_end(tmp.path()), Cursor::default());
        let a = note(tmp.path(), 30, "A");
        a.append("## 1 · 14:30\n\nOld.").unwrap();
        let mut cursor = Cursor::at_end(tmp.path());
        assert_eq!(cursor, Cursor { note: Some(a.dir.clone()), entry: 1, len: 4 });
        assert!(collect(tmp.path(), &cursor, None).is_empty());

        a.append("![14:31:00](images/01-143100.png)").unwrap();
        a.append("## 2 · 14:32\n\nNew.").unwrap();
        let b = note(tmp.path(), 40, "B");
        b.append("## 1 · 14:40\n\nOther note.").unwrap();
        let items = collect(tmp.path(), &cursor, None);
        assert_eq!(numbers(&items), ["A #1 +![14:31:00](images/01-143100.png)", "A #2", "B #1"]);

        for item in &items {
            cursor.advance(item);
        }
        assert!(collect(tmp.path(), &cursor, None).is_empty());
        b.append("## 2 · 14:41\n\nLatest.").unwrap();
        assert_eq!(numbers(&collect(tmp.path(), &cursor, None)), ["B #2"]);
    }

    #[test]
    fn stops_at_the_batch_end() {
        let tmp = tempfile::tempdir().unwrap();
        let a = note(tmp.path(), 30, "A");
        let cursor = Cursor::at_end(tmp.path());
        a.append("## 1 · 14:30\n\nOne.").unwrap();
        a.append("## 2 · 14:31\n\nTwo.").unwrap();
        let b = note(tmp.path(), 40, "B");
        b.append("## 1 · 14:40\n\nThree.").unwrap();
        assert_eq!(numbers(&collect(tmp.path(), &cursor, Some((&a.dir, 1)))), ["A #1"]);
        assert_eq!(numbers(&collect(tmp.path(), &cursor, Some((&a.dir, 2)))), ["A #1", "A #2"]);
        assert_eq!(numbers(&collect(tmp.path(), &cursor, Some((&b.dir, 1)))), ["A #1", "A #2", "B #1"]);
    }

    #[test]
    fn makes_links_absolute() {
        let dir = Path::new("/r/.asides/2026-10-15-1432 Checkout");
        let text = "Here.\n\n![14:38:51](images/05-143851.png)\n\n[docs](https://x.dev/a) and ![x](/abs.png)";
        assert_eq!(
            absolute_links(dir, text),
            "Here.\n\n![14:38:51](</r/.asides/2026-10-15-1432 Checkout/images/05-143851.png>)\n\n\
             [docs](https://x.dev/a) and ![x](/abs.png)"
        );
        assert_eq!(
            absolute_links(dir, "5 · 14:38 · [audio](audio/05.m4a)"),
            format!("5 · 14:38 · [audio](<{}>)", dir.join("audio/05.m4a").display())
        );
        assert_eq!(image_paths(dir, text), [dir.join("images/05-143851.png")]);
        assert_eq!(absolute_links(dir, "a ](unclosed"), "a ](unclosed");
    }

    #[test]
    fn listener_records_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(Listener::read_all(&tmp.path().join("none")).is_empty());
        let listener = Listener { pid: 42, client: "Codex".into(), dir: "/r".into(), waiting: true, updated_ms: 1 };
        listener.write(tmp.path()).unwrap();
        assert_eq!(Listener::read_all(tmp.path()), [listener]);
        fs::remove_file(Listener::file(tmp.path(), 42)).unwrap();
        assert!(Listener::read_all(tmp.path()).is_empty());
    }

    #[test]
    fn labels_clients() {
        assert_eq!(client_label("claude-code"), "Claude Code");
        assert_eq!(client_label("codex-mcp-client"), "Codex");
        assert_eq!(client_label("opencode"), "OpenCode");
        assert_eq!(client_label(""), "Agent");
        assert_eq!(client_label("Zed"), "Zed");
    }
}
