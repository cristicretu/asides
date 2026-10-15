//! Notes live in `<bound dir>/.asides/<YYYY-MM-DD-HHMM Title>/notes.md`, one folder per note.

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};

use crate::paths::{NOTE_FILE, NOTES_DIR};

const MAX_TITLE_CHARS: usize = 60;
/// Length of the `YYYY-MM-DD-HHMM` folder-name prefix.
const STAMP_LEN: usize = 15;

/// One entry of a note: its `## N · HH:MM` heading and everything up to the next entry.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub number: u32,
    /// The heading without its `## `, e.g. `5 · 14:38 · [audio](audio/05-143812.m4a)`.
    pub heading: String,
    /// The transcript, images and anything added by hand, without surrounding blank lines.
    pub body: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub dir: PathBuf,
    pub title: String,
}

impl Note {
    pub fn file(&self) -> PathBuf {
        self.dir.join(NOTE_FILE)
    }

    pub fn entry_count(&self) -> u32 {
        fs::read_to_string(self.file()).map(|c| entry_numbers(&c).count() as u32).unwrap_or(0)
    }

    pub fn next_entry_number(&self) -> u32 {
        fs::read_to_string(self.file()).map(|c| entry_numbers(&c).max().unwrap_or(0) + 1).unwrap_or(1)
    }

    /// Appends `block` as its own paragraph group, separated from what precedes it by a blank line.
    pub fn append(&self, block: &str) -> io::Result<()> {
        let mut file = fs::OpenOptions::new().read(true).append(true).open(self.file())?;
        let len = file.metadata()?.len();
        let mut tail = [0u8; 2];
        let take = len.min(2) as usize;
        if take > 0 {
            file.seek(SeekFrom::End(-(take as i64)))?;
            file.read_exact(&mut tail[..take])?;
        }
        let sep = match (take, &tail[..take]) {
            (0, _) => "",
            (_, [.., b'\n', b'\n']) => "",
            (_, [.., b'\n']) => "\n",
            _ => "\n\n",
        };
        let block = block.trim_end_matches('\n');
        file.write_all(format!("{sep}{block}\n").as_bytes())
    }
}

pub fn notes_root(base: &Path) -> PathBuf {
    base.join(NOTES_DIR)
}

/// All notes under `base`, oldest first.
pub fn list_notes(base: &Path) -> Vec<Note> {
    let Ok(entries) = fs::read_dir(notes_root(base)) else {
        return Vec::new();
    };
    let mut notes: Vec<(String, std::time::SystemTime, Note)> = entries
        .flatten()
        .filter(|e| e.path().join(NOTE_FILE).is_file())
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let created = e.metadata().and_then(|m| m.created()).unwrap_or(std::time::UNIX_EPOCH);
            let title = title_from_folder_name(&name);
            (name.chars().take(STAMP_LEN).collect(), created, Note { dir: e.path(), title })
        })
        .collect();
    notes.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    notes.into_iter().map(|(_, _, note)| note).collect()
}

/// The note new recordings append to: the most recently created one.
pub fn latest_note(base: &Path) -> Option<Note> {
    list_notes(base).pop()
}

/// Creates the note folder and writes `header` (the YAML front matter) to its `notes.md`.
pub fn create_note(base: &Path, created: DateTime<Local>, title: &str, header: &str) -> io::Result<Note> {
    let root = notes_root(base);
    fs::create_dir_all(&root)?;
    let title = sanitize_title(title);
    let stem = format!("{} {}", created.format("%Y-%m-%d-%H%M"), title);
    let mut dir = root.join(&stem);
    let mut n = 2;
    while dir.exists() {
        dir = root.join(format!("{stem} {n}"));
        n += 1;
    }
    fs::create_dir(&dir)?;
    let mut content = header.trim_end_matches('\n').to_string();
    content.push('\n');
    fs::write(dir.join(NOTE_FILE), content)?;
    Ok(Note { dir, title })
}

/// Makes `raw` safe and short enough for a folder name; never returns an empty string.
pub fn sanitize_title(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' => '-',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let mut title = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    title = title.trim_start_matches('.').trim().to_string();
    if title.chars().count() > MAX_TITLE_CHARS {
        let cut: String = title.chars().take(MAX_TITLE_CHARS + 1).collect();
        title = match cut.rfind(' ') {
            Some(i) if i > MAX_TITLE_CHARS / 2 => cut[..i].to_string(),
            _ => cut.chars().take(MAX_TITLE_CHARS).collect(),
        };
    }
    let title = title.trim_end_matches(|c: char| c.is_whitespace() || ".,;!?-".contains(c));
    if title.is_empty() { "Untitled".into() } else { title.to_string() }
}

/// The first sentence of a transcript, used to name a note when no title was given.
pub fn title_from_transcript(text: &str) -> String {
    let text = text.trim();
    let mut end = text.len();
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '\n' {
            end = i;
            break;
        }
        if ".!?".contains(c) && chars.peek().is_none_or(|(_, next)| next.is_whitespace()) {
            end = i;
            break;
        }
    }
    sanitize_title(&text[..end])
}

/// Zips the note folder (notes.md, images, audio) for sharing. Entry names are UTF-8 flagged, so
/// diacritics survive on every OS, and macOS metadata files (`.DS_Store`, `._*`) are left out.
pub fn export_zip(note: &Note, out: &Path) -> io::Result<()> {
    let root = note.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "note".into());
    let mut zip = zip::ZipWriter::new(File::create(out)?);
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    add_to_zip(&mut zip, &note.dir, &root, options)?;
    zip.finish().map_err(io::Error::other)?;
    Ok(())
}

fn add_to_zip(
    zip: &mut zip::ZipWriter<File>,
    dir: &Path,
    prefix: &str,
    options: zip::write::SimpleFileOptions,
) -> io::Result<()> {
    zip.add_directory(format!("{prefix}/"), options).map_err(io::Error::other)?;
    let mut entries: Vec<_> = fs::read_dir(dir)?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let zip_name = format!("{prefix}/{name}");
        if path.is_dir() {
            add_to_zip(zip, &path, &zip_name, options)?;
        } else {
            zip.start_file(zip_name, options).map_err(io::Error::other)?;
            io::copy(&mut File::open(&path)?, zip)?;
        }
    }
    Ok(())
}

fn title_from_folder_name(name: &str) -> String {
    match name.get(STAMP_LEN..) {
        Some(rest) if name.as_bytes().get(4) == Some(&b'-') => rest.trim().to_string(),
        _ => name.to_string(),
    }
}

/// Numbers of entry headings (`## 3 · 14:38`) in a note.
fn entry_numbers(content: &str) -> impl Iterator<Item = u32> + '_ {
    content.lines().filter_map(entry_number)
}

fn entry_number(line: &str) -> Option<u32> {
    let rest = line.strip_prefix("## ")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// The entries of a note, in file order. Text before the first entry (the front matter) is skipped.
pub fn parse_entries(content: &str) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut body: Vec<&str> = Vec::new();
    let finish = |entries: &mut Vec<Entry>, body: &mut Vec<&str>| {
        if let Some(entry) = entries.last_mut() {
            entry.body = body.join("\n").trim_matches('\n').trim_end().to_string();
        }
        body.clear();
    };
    for line in content.lines() {
        if let Some(number) = entry_number(line) {
            finish(&mut entries, &mut body);
            entries.push(Entry { number, heading: line[3..].trim_end().to_string(), body: String::new() });
        } else if !entries.is_empty() {
            body.push(line);
        }
    }
    finish(&mut entries, &mut body);
    entries
}

/// The YAML front matter between the opening `---` and the closing one, without the markers.
pub fn front_matter(content: &str) -> Option<&str> {
    let rest = content.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    Some(&rest[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(h: u32, m: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 10, 15, h, m, 0).unwrap()
    }

    #[test]
    fn sanitizes_titles() {
        assert_eq!(sanitize_title("  Checkout / cart: bugs?  "), "Checkout - cart- bugs");
        assert_eq!(sanitize_title("..hidden"), "hidden");
        assert_eq!(sanitize_title("   "), "Untitled");
        assert_eq!(sanitize_title("Șterge butonul de închidere"), "Șterge butonul de închidere");
        let long = "Debugging the checkout flow and the cart drawer thoughts on the UI for the payment step";
        let t = sanitize_title(long);
        assert!(t.chars().count() <= 60, "{t}");
        assert_eq!(t, "Debugging the checkout flow and the cart drawer thoughts on");
    }

    #[test]
    fn titles_from_first_sentence() {
        assert_eq!(
            title_from_transcript(" Debugging the checkout flow. Thoughts on the cart UI."),
            "Debugging the checkout flow"
        );
        assert_eq!(title_from_transcript("Versiunea 2.5 are un bug! Restul"), "Versiunea 2.5 are un bug");
        assert_eq!(title_from_transcript(""), "Untitled");
    }

    #[test]
    fn latest_note_is_newest_created() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(latest_note(tmp.path()), None);
        create_note(tmp.path(), at(9, 5), "Morning", "---\n---").unwrap();
        create_note(tmp.path(), at(14, 32), "Checkout session", "---\n---").unwrap();
        create_note(tmp.path(), at(11, 0), "Late-added older", "---\n---").unwrap();
        let latest = latest_note(tmp.path()).unwrap();
        assert_eq!(latest.title, "Checkout session");
        assert!(latest.dir.ends_with(".asides/2026-10-15-1432 Checkout session"));
    }

    #[test]
    fn same_minute_same_title_gets_suffix() {
        let tmp = tempfile::tempdir().unwrap();
        let a = create_note(tmp.path(), at(14, 32), "Bugs", "h").unwrap();
        let b = create_note(tmp.path(), at(14, 32), "Bugs", "h").unwrap();
        assert_ne!(a.dir, b.dir);
        assert!(b.dir.ends_with("2026-10-15-1432 Bugs 2"));
        assert_eq!(latest_note(tmp.path()).unwrap().dir, b.dir);
    }

    #[test]
    fn appends_with_blank_lines_and_counts_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let note = create_note(tmp.path(), at(14, 32), "S", "---\ntitle: S\n---").unwrap();
        assert_eq!(note.next_entry_number(), 1);
        note.append("## 1 · 14:32\n\nHello.").unwrap();
        // Simulate an external editor leaving no trailing newline.
        let mut c = fs::read_to_string(note.file()).unwrap();
        c.push_str("edited by hand");
        fs::write(note.file(), &c).unwrap();
        note.append("## 2 · 14:40\n\nSecond.").unwrap();
        let content = fs::read_to_string(note.file()).unwrap();
        assert_eq!(
            content,
            "---\ntitle: S\n---\n\n## 1 · 14:32\n\nHello.\nedited by hand\n\n## 2 · 14:40\n\nSecond.\n"
        );
        assert_eq!(note.entry_count(), 2);
        assert_eq!(note.next_entry_number(), 3);
    }

    #[test]
    fn exports_zip_with_utf8_names_and_no_macos_metadata() {
        let tmp = tempfile::tempdir().unwrap();
        let note = create_note(tmp.path(), at(17, 26), "Butonul după schimbare", "---\n---").unwrap();
        fs::create_dir(note.dir.join("images")).unwrap();
        fs::write(note.dir.join("images/01-172600.png"), [0x89, b'P', b'N', b'G']).unwrap();
        fs::write(note.dir.join(".DS_Store"), "x").unwrap();
        fs::write(note.dir.join("._notes.md"), "x").unwrap();
        let out = tmp.path().join("note.zip");
        export_zip(&note, &out).unwrap();

        let mut archive = zip::ZipArchive::new(File::open(&out).unwrap()).unwrap();
        let mut names: Vec<String> = archive.file_names().map(|n| n.unwrap().into_owned()).collect();
        names.sort();
        let root = "2026-10-15-1726 Butonul după schimbare";
        assert_eq!(
            names,
            [
                format!("{root}/"),
                format!("{root}/images/"),
                format!("{root}/images/01-172600.png"),
                format!("{root}/notes.md")
            ]
        );
        let mut content = String::new();
        archive.by_name(&format!("{root}/notes.md")).unwrap().read_to_string(&mut content).unwrap();
        assert_eq!(content, "---\n---\n");
    }

    #[test]
    fn parses_entries_and_front_matter() {
        let content = "---\ntitle: S\ndirectory: /x\n---\n\n## 1 · 14:32\n\nHello.\n\n## Summary by hand\n\n\
                       ## 2 · 14:40 · [audio](audio/02.m4a)\n\nOne.\n\n![14:40:02](images/02.png)\n\nTwo.\n\n\n";
        let entries = parse_entries(content);
        assert_eq!(
            entries,
            [
                Entry { number: 1, heading: "1 · 14:32".into(), body: "Hello.\n\n## Summary by hand".into() },
                Entry {
                    number: 2,
                    heading: "2 · 14:40 · [audio](audio/02.m4a)".into(),
                    body: "One.\n\n![14:40:02](images/02.png)\n\nTwo.".into()
                },
            ]
        );
        assert_eq!(front_matter(content), Some("title: S\ndirectory: /x"));
        assert_eq!(front_matter("## 1 · 10:00\n"), None);
        assert_eq!(parse_entries("---\n---\n"), []);
    }

    #[test]
    fn ignores_non_entry_headings() {
        let numbers: Vec<u32> = entry_numbers("## Summary\n## 4 · 10:00\n### 9 nope\n## 12 · x").collect();
        assert_eq!(numbers, vec![4, 12]);
    }
}
