//! `asides mcp`: a Model Context Protocol server (JSON-RPC over stdio) that hands the user's
//! asides to a coding agent (Claude Code, Codex, OpenCode, …) as prompts. Agents wait for new
//! entries with `wait_for_asides`: each one as soon as it's transcribed, or in batches when the
//! user chooses Send to Agent (`agent_delivery` in config.toml). `get_asides` reads a whole note.
//!
//! The server only reads notes and the app's published state, so both tools are read-only (agents
//! such as Codex run those without asking). Its one effect on the app is `bind_asides`, which binds
//! it to this repo like running `asides` here.

use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use asides_core::agents::{self, Cursor, Item, Listener};
use asides_core::config::{AgentDelivery, Config};
use asides_core::link::Link;
use asides_core::notes::{self, Note};
use asides_core::paths::{self, tilde};
use asides_core::project;
use asides_core::state::AppState;
use serde_json::{Value, json};

/// Protocol revisions this server speaks, oldest first; a client asking for another gets the newest.
const PROTOCOL_VERSIONS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"];
const POLL: Duration = Duration::from_millis(400);
const PROGRESS_EVERY: Duration = Duration::from_secs(20);
/// Most clients give up on a tool call after 60 s (Codex, and the MCP SDKs' default), so a wait
/// returns before that unless the client is known to allow more or `--timeout` says otherwise.
const DEFAULT_WAIT: Duration = Duration::from_secs(50);
const CLAUDE_CODE_WAIT: Duration = Duration::from_secs(25 * 60);
const MAX_WAIT_SECS: u64 = 3600;
/// Screenshots larger than this are only linked; image content has size limits in most clients.
const MAX_INLINE_IMAGE: u64 = 3_750_000;
const MAX_INLINE_IMAGES: usize = 6;

const INSTRUCTIONS: &str = "Asides lets the user talk while they test their app: they press a shortcut, \
speak (a bug, a request, an idea), and Asides transcribes it on their Mac and appends it to a Markdown \
note in this repo (.asides/<date title>/notes.md), with screenshots inline. Each aside is a request from \
the user. When the user asks you to listen to their asides, call wait_for_asides, act on what you get, \
then call it again; keep listening until they tell you to stop. If it says Asides is bound to another \
directory or isn't running, call bind_asides so recordings come to this repo. When they ask you to work \
through their notes, call get_asides. Transcripts are verbatim speech-to-text: expect misheard words, \
especially names of projects, files and libraries.";

const LISTEN_PROMPT: &str = "Listen to my asides. Call the wait_for_asides tool; whenever it returns asides, \
treat each one as a request from me and act on it, then call wait_for_asides again. If it returns none, \
call it again straight away. Keep listening until I tell you to stop.";

/// Where the server stands: what this agent has been given so far.
struct Session {
    dir: PathBuf,
    client: String,
    /// Wait in seconds when a call doesn't say; `None` picks one for the client.
    timeout: Option<u64>,
    cursor: Cursor,
    /// The last Send to Agent already acted on (`AgentSend::at_ms`).
    last_send: u64,
    waits: usize,
}

struct Server {
    session: Mutex<Session>,
    out: Mutex<io::Stdout>,
    running: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

pub fn run(dir: PathBuf, timeout: Option<u64>) -> Result<()> {
    let last_send = AppState::read(&paths::state_path()).and_then(|s| s.agent_send).map_or(0, |s| s.at_ms);
    let server = Arc::new(Server {
        session: Mutex::new(Session {
            cursor: Cursor::at_end(&dir),
            dir,
            client: String::new(),
            timeout,
            last_send,
            waits: 0,
        }),
        out: Mutex::new(io::stdout()),
        running: Mutex::new(HashMap::new()),
    });
    for line in io::stdin().lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(&line) {
            Ok(Value::Array(batch)) => batch.into_iter().for_each(|m| server.dispatch(m)),
            Ok(message) => server.dispatch(message),
            Err(err) => server.send(&json!({
                "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": format!("parse error: {err}") }
            })),
        }
    }
    // The client hung up: stop waiting and stop showing as listening.
    for cancel in server.running.lock().unwrap().values() {
        cancel.store(true, Ordering::Relaxed);
    }
    let _ = fs::remove_file(Listener::file(&paths::listeners_dir(), std::process::id()));
    Ok(())
}

impl Server {
    fn send(&self, message: &Value) {
        let mut out = self.out.lock().unwrap();
        let _ = writeln!(out, "{message}");
        let _ = out.flush();
    }

    fn reply(&self, id: &Value, result: Result<Value, (i64, String)>) {
        let message = match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message)) => {
                json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
            }
        };
        self.send(&message);
    }

    fn dispatch(self: &Arc<Self>, message: Value) {
        let method = message.get("method").and_then(Value::as_str).unwrap_or_default().to_string();
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let Some(id) = message.get("id").cloned() else {
            // A notification (or a response to a request this server never makes).
            if method == "notifications/cancelled"
                && let Some(request) = params.get("requestId")
                && let Some(cancel) = self.running.lock().unwrap().get(&request.to_string())
            {
                cancel.store(true, Ordering::Relaxed);
            }
            return;
        };
        let result = match method.as_str() {
            "initialize" => Ok(self.initialize(&params)),
            "ping" | "logging/setLevel" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "prompts/list" => Ok(json!({ "prompts": prompts() })),
            "prompts/get" => self.prompt(&params),
            "tools/call" => return self.call_tool(id, &params),
            _ => Err((-32601, format!("method not found: {method}"))),
        };
        self.reply(&id, result);
    }

    fn initialize(&self, params: &Value) -> Value {
        let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or_default();
        let version = PROTOCOL_VERSIONS.iter().find(|v| **v == asked).unwrap_or(PROTOCOL_VERSIONS.last().unwrap());
        let client = params.pointer("/clientInfo/name").and_then(Value::as_str).unwrap_or_default();
        self.session.lock().unwrap().client = client.to_string();
        json!({
            "protocolVersion": version,
            "capabilities": { "tools": {}, "prompts": {} },
            "serverInfo": { "name": "asides", "title": "Asides", "version": env!("CARGO_PKG_VERSION") },
            "instructions": INSTRUCTIONS,
        })
    }

    fn prompt(&self, params: &Value) -> Result<Value, (i64, String)> {
        let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
        let text = match name {
            "listen" => LISTEN_PROMPT.to_string(),
            "asides" => {
                let mut session = self.session.lock().unwrap();
                let dir = session.dir.clone();
                let text = read_note(&dir, None, &mut session.cursor).map_err(|e| (-32602, e))?;
                format!(
                    "Work through my asides below. I recorded them while testing; they were transcribed \
                     verbatim, so expect misheard words. Treat each entry as a request from me.\n\n{text}"
                )
            }
            _ => return Err((-32602, format!("unknown prompt: {name}"))),
        };
        Ok(json!({ "messages": [{ "role": "user", "content": { "type": "text", "text": text } }] }))
    }

    fn call_tool(self: &Arc<Self>, id: Value, params: &Value) {
        let name = params.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
        let args = params.get("arguments").cloned().unwrap_or(json!({}));
        match name.as_str() {
            "get_asides" => {
                let after = args.get("after_entry").and_then(Value::as_u64).map(|n| n as u32);
                let mut session = self.session.lock().unwrap();
                let dir = session.dir.clone();
                let result = match read_note(&dir, after, &mut session.cursor) {
                    Ok(text) => tool_text(text, false),
                    Err(err) => tool_text(err, true),
                };
                drop(session);
                self.reply(&id, Ok(result));
            }
            "bind_asides" => {
                // Binding waits for the app to answer (up to 15 s): not on the thread reading requests.
                let server = self.clone();
                std::thread::spawn(move || {
                    let dir = server.session.lock().unwrap().dir.clone();
                    let result = match bind_here(&dir) {
                        Ok(true) => tool_text(
                            format!("Asides is now bound to {}: the user's recordings go to this repo.", tilde(&dir)),
                            false,
                        ),
                        Ok(false) => tool_text(format!("Asides was already bound to {}.", tilde(&dir)), false),
                        Err(err) => tool_text(
                            format!(
                                "Could not bind Asides: {err:#}. The user can run `asides` in {} instead.",
                                tilde(&dir)
                            ),
                            true,
                        ),
                    };
                    server.reply(&id, Ok(result));
                });
            }
            "wait_for_asides" => {
                let cancel = Arc::new(AtomicBool::new(false));
                self.running.lock().unwrap().insert(id.to_string(), cancel.clone());
                let progress = params.pointer("/_meta/progressToken").cloned();
                let timeout = args.get("timeout_seconds").and_then(Value::as_u64);
                let server = self.clone();
                std::thread::spawn(move || {
                    let result = server.wait(timeout, progress, &cancel);
                    server.running.lock().unwrap().remove(&id.to_string());
                    // A cancelled request gets no response.
                    if let Some(result) = result {
                        server.reply(&id, Ok(result));
                    }
                });
            }
            _ => self.reply(&id, Ok(tool_text(format!("Unknown tool: {name}"), true))),
        }
    }

    /// Waits until there's something to deliver or the time is up; `None` when cancelled.
    fn wait(&self, timeout: Option<u64>, progress: Option<Value>, cancel: &AtomicBool) -> Option<Value> {
        let dir = self.session.lock().unwrap().dir.clone();
        // No point waiting for recordings that can't come here.
        if let Some(problem) = binding_problem(&dir) {
            return Some(tool_text(
                format!(
                    "{problem} If the user wants to record asides for this repo, call bind_asides (or ask them \
                     to run `asides` in {}), then call wait_for_asides again.",
                    tilde(&dir)
                ),
                false,
            ));
        }
        self.set_waiting(1);
        let limit = {
            let session = self.session.lock().unwrap();
            let default = match session.timeout {
                Some(secs) => Duration::from_secs(secs),
                None if session.client.to_ascii_lowercase().contains("claude") => CLAUDE_CODE_WAIT,
                None => DEFAULT_WAIT,
            };
            timeout.map(|s| Duration::from_secs(s.clamp(1, MAX_WAIT_SECS))).unwrap_or(default)
        };
        let started = Instant::now();
        let mut next_progress = started + PROGRESS_EVERY;
        let mut steps = 0;
        let result = loop {
            if cancel.load(Ordering::Relaxed) {
                break None;
            }
            let items = self.take_new();
            if !items.is_empty() {
                break Some(delivery(&items));
            }
            if started.elapsed() >= limit {
                let mut text = format!(
                    "No new asides yet (waited {} s). Call wait_for_asides again to keep listening.",
                    limit.as_secs()
                );
                for notice in app_notices(&dir) {
                    text.push_str(&format!("\n\n{notice}"));
                }
                break Some(tool_text(text, false));
            }
            if let Some(token) = &progress
                && Instant::now() >= next_progress
            {
                steps += 1;
                next_progress += PROGRESS_EVERY;
                self.send(&json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/progress",
                    "params": { "progressToken": token, "progress": steps, "message": "Listening for asides…" },
                }));
            }
            std::thread::sleep(POLL);
        };
        self.set_waiting(-1);
        result
    }

    /// Counts the waits in progress and publishes the listener record the app shows.
    fn set_waiting(&self, change: isize) {
        let mut session = self.session.lock().unwrap();
        session.waits = session.waits.saturating_add_signed(change);
        let listener = Listener {
            pid: std::process::id(),
            client: agents::client_label(&session.client),
            dir: session.dir.clone(),
            waiting: session.waits > 0,
            updated_ms: crate::now_ms(),
        };
        let _ = listener.write(&paths::listeners_dir());
    }

    /// What's ready for the agent now, given the delivery setting, and moves past it.
    fn take_new(&self) -> Vec<Item> {
        let config = Config::load(&paths::config_path()).unwrap_or_default();
        let mut session = self.session.lock().unwrap();
        let items = match config.agent_delivery {
            AgentDelivery::Each => agents::collect(&session.dir, &session.cursor, None),
            AgentDelivery::Batch => {
                let send = AppState::read(&paths::state_path())
                    .and_then(|s| s.agent_send)
                    .filter(|send| send.at_ms > session.last_send && send.dir == session.dir);
                match send {
                    Some(send) => {
                        session.last_send = send.at_ms;
                        match &send.note {
                            Some(note) => agents::collect(&session.dir, &session.cursor, Some((note, send.entry))),
                            None => Vec::new(),
                        }
                    }
                    None => Vec::new(),
                }
            }
        };
        for item in &items {
            session.cursor.advance(item);
        }
        items
    }
}

/// Binds the app to `dir` unless it's already bound there. `Ok(true)` when it bound it.
fn bind_here(dir: &Path) -> Result<bool> {
    if crate::running_state().is_some_and(|s| s.bound_dir.as_deref() == Some(dir)) {
        return Ok(false);
    }
    let _ = project::ensure_git_exclude(dir);
    let request = crate::request_id();
    crate::send(&Link::Bind { dir: dir.to_path_buf(), new_note: false, title: None, request: Some(request.clone()) })?;
    crate::wait_for_ack(&request)?;
    Ok(true)
}

/// Why the user's recordings wouldn't reach this repo, if they wouldn't.
fn binding_problem(dir: &Path) -> Option<String> {
    let Some(state) = crate::running_state() else {
        return Some("Asides isn't running, so nothing can be recorded yet.".into());
    };
    match &state.bound_dir {
        Some(bound) if bound == dir => None,
        Some(bound) => {
            Some(format!("Asides is bound to {}, so the user's recordings go there, not to this repo.", tilde(bound)))
        }
        None => Some("Asides isn't bound to a directory yet.".into()),
    }
}

/// Why asides might not be arriving, for the agent to pass on.
fn app_notices(dir: &Path) -> Vec<String> {
    if let Some(problem) = binding_problem(dir) {
        return vec![format!("{problem} Call bind_asides to bind it to this repo.")];
    }
    let Some(state) = crate::running_state() else { return Vec::new() };
    let mut notices = Vec::new();
    match state.status.as_str() {
        "recording" => notices.push("The user is recording an aside right now.".into()),
        "transcribing" => notices.push("An aside is being transcribed.".into()),
        "error" => notices.push(format!("Asides reports a problem: {}", state.detail.unwrap_or_default())),
        _ => {}
    }
    if Config::load(&paths::config_path()).unwrap_or_default().agent_delivery == AgentDelivery::Batch {
        notices.push("The user gets asides to you in batches: they arrive when the user chooses Send to Agent.".into());
    }
    notices
}

/// The tool result for new asides: their text, grouped by note, then the screenshots they show.
fn delivery(items: &[Item]) -> Value {
    let count = items.iter().filter(|i| matches!(i, Item::Entry { .. })).count();
    let mut text = match count {
        0 => "The user added to asides you already have.".to_string(),
        1 => "A new aside from the user.".to_string(),
        n => format!("{n} new asides from the user."),
    };
    text.push_str(
        " They were spoken while testing and transcribed verbatim (expect misheard words); treat each as a \
         request from the user.",
    );
    let mut images = Vec::new();
    let mut current: Option<&Note> = None;
    for item in items {
        let (note, heading, body) = match item {
            Item::Entry { note, entry } => (note, entry.heading.clone(), entry.body.as_str()),
            Item::Addition { note, number, text } => (note, format!("{number} (added since)"), text.as_str()),
        };
        if current.is_none_or(|c| c.dir != note.dir) {
            text.push_str(&format!("\n\nNote \"{}\" ({}):", note.title, note.file().display()));
            current = Some(note);
        }
        let heading = agents::absolute_links(&note.dir, &heading);
        text.push_str(&format!("\n\n## {heading}\n\n{}", agents::absolute_links(&note.dir, body.trim())));
        images.extend(agents::image_paths(&note.dir, body));
    }
    text.push_str("\n\nWhen you've handled them, call wait_for_asides again to keep listening.");
    let mut content = vec![json!({ "type": "text", "text": text })];
    content.extend(images.iter().filter_map(|path| inline_image(path)).take(MAX_INLINE_IMAGES));
    json!({ "content": content, "isError": false })
}

/// The latest note with absolute links, or only its entries after `after`; moves `cursor` past them.
fn read_note(dir: &Path, after: Option<u32>, cursor: &mut Cursor) -> Result<String, String> {
    let note = notes::latest_note(dir).ok_or_else(|| format!("There are no asides in {} yet.", tilde(dir)))?;
    let content =
        fs::read_to_string(note.file()).map_err(|e| format!("Could not read {}: {e}", note.file().display()))?;
    let entries: Vec<_> =
        notes::parse_entries(&content).into_iter().filter(|e| after.is_none_or(|after| e.number > after)).collect();
    let mut text = format!("Note \"{}\" ({})", note.title, note.file().display());
    if let Some(meta) = notes::front_matter(&content) {
        text.push_str(&format!("\n\n---\n{meta}\n---"));
    }
    if entries.is_empty() {
        text.push_str("\n\nNo entries yet.");
    }
    for entry in &entries {
        let heading = agents::absolute_links(&note.dir, &entry.heading);
        text.push_str(&format!("\n\n## {heading}\n\n{}", agents::absolute_links(&note.dir, &entry.body)));
    }
    if content.contains("](images/") {
        text.push_str("\n\nScreenshots are linked by absolute path; open them to see what the user saw.");
    }
    // Don't hand these to a listening agent again.
    if let Some(last) = entries.last() {
        let later_note = cursor.note.as_ref().is_none_or(|c| c.file_name() < note.dir.file_name());
        if later_note || (cursor.note.as_ref() == Some(&note.dir) && last.number >= cursor.entry) {
            *cursor = Cursor { note: Some(note.dir.clone()), entry: last.number, len: last.body.len() };
        }
    }
    Ok(text)
}

fn inline_image(path: &Path) -> Option<Value> {
    let mime = match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return None,
    };
    if fs::metadata(path).ok()?.len() > MAX_INLINE_IMAGE {
        return None;
    }
    Some(json!({ "type": "image", "data": base64(&fs::read(path).ok()?), "mimeType": mime }))
}

fn tool_text(text: String, is_error: bool) -> Value {
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

fn tools() -> Value {
    json!([
        {
            "name": "wait_for_asides",
            "title": "Wait for asides",
            "description": "Wait for the user's next spoken asides in this repo and return them. The user records \
                asides with a global shortcut while they test (bugs, requests, ideas); Asides transcribes them on \
                their Mac. Each aside is a request from the user: act on it, then call this tool again to keep \
                listening. Asides arrive one at a time as they're transcribed, or in a batch when the user chooses \
                Send to Agent, depending on their setting. Returns with no asides when the wait times out; call it \
                again to keep listening. Screenshots the user attached come back as images and absolute paths.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "timeout_seconds": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": MAX_WAIT_SECS,
                        "description": "How long to wait before returning with no asides. Leave it out to use a \
                            wait that suits your client's tool timeout."
                    }
                }
            },
            "annotations": { "title": "Wait for asides", "readOnlyHint": true, "openWorldHint": false }
        },
        {
            "name": "get_asides",
            "title": "Read the latest asides note",
            "description": "Read the user's latest asides note in this repo: its metadata (git branch and commit, \
                browser, display) and its entries, with absolute paths to screenshots. Use it when the user asks \
                you to work through their asides or notes. Pass after_entry to get only later entries.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "after_entry": {
                        "type": "integer",
                        "minimum": 0,
                        "description": "Only return entries numbered higher than this."
                    }
                }
            },
            "annotations": { "title": "Read the latest asides note", "readOnlyHint": true, "openWorldHint": false }
        },
        {
            "name": "bind_asides",
            "title": "Bind Asides to this repo",
            "description": "Point the Asides app at this repo, so the user's recordings go to its notes and \
                wait_for_asides can return them. Starts the app if it isn't running. Same as the user running \
                `asides` here. Call it when wait_for_asides says Asides is bound elsewhere or isn't running.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": {
                "title": "Bind Asides to this repo",
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            }
        }
    ])
}

fn prompts() -> Value {
    json!([
        {
            "name": "listen",
            "title": "Listen to my asides",
            "description": "Act on each aside as the user records it, until they say stop."
        },
        {
            "name": "asides",
            "title": "Work through my asides",
            "description": "Work through the latest asides note in this repo."
        }
    ])
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n =
            (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, TimeZone};

    #[test]
    fn encodes_base64() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xff, 0xfe, 0x00, 0x01]), "//4AAQ==");
    }

    #[test]
    fn delivers_entries_grouped_by_note_with_images() {
        let tmp = tempfile::tempdir().unwrap();
        let at = Local.with_ymd_and_hms(2026, 10, 15, 14, 32, 0).unwrap();
        let note = notes::create_note(tmp.path(), at, "Checkout", "---\ntitle: Checkout\n---").unwrap();
        fs::create_dir(note.dir.join("images")).unwrap();
        fs::write(note.dir.join("images/01-143300.png"), [0x89, b'P', b'N', b'G']).unwrap();
        note.append("## 1 · 14:32\n\nThe card.\n\n![14:33:00](images/01-143300.png)").unwrap();
        let items = agents::collect(tmp.path(), &Cursor::default(), None);
        let result = delivery(&items);
        let text = result.pointer("/content/0/text").and_then(Value::as_str).unwrap();
        assert!(text.starts_with("A new aside from the user."), "{text}");
        assert!(text.contains(&format!("Note \"Checkout\" ({})", note.file().display())), "{text}");
        assert!(
            text.contains(&format!(
                "## 1 · 14:32\n\nThe card.\n\n![14:33:00](<{}>)",
                note.dir.join("images/01-143300.png").display()
            )),
            "{text}"
        );
        assert_eq!(result.pointer("/content/1/type"), Some(&json!("image")));
        assert_eq!(result.pointer("/content/1/data"), Some(&json!("iVBORw==")));
    }

    #[test]
    fn reading_a_note_moves_the_cursor_past_it() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cursor = Cursor::default();
        assert!(read_note(tmp.path(), None, &mut cursor).unwrap_err().contains("no asides"));
        let at = Local.with_ymd_and_hms(2026, 10, 15, 14, 32, 0).unwrap();
        let note = notes::create_note(tmp.path(), at, "S", "---\ntitle: S\nbranch: main\n---").unwrap();
        note.append("## 1 · 14:32\n\nOne.").unwrap();
        note.append("## 2 · 14:33\n\nTwo.").unwrap();
        let text = read_note(tmp.path(), Some(1), &mut cursor).unwrap();
        assert!(text.contains("---\ntitle: S\nbranch: main\n---") && text.contains("## 2 · 14:33\n\nTwo."), "{text}");
        assert!(!text.contains("One."));
        assert_eq!(cursor, Cursor { note: Some(note.dir.clone()), entry: 2, len: 4 });
        assert!(agents::collect(tmp.path(), &cursor, None).is_empty());
    }
}
