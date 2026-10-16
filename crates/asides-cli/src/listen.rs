//! `asides listen`: the user's asides on standard output as they arrive, for agents that watch a
//! command in the background instead of waiting in a tool call (Claude Code's Monitor tool turns
//! each burst of output into a notification in the chat). The agent stays free between asides.
//!
//! Delivery follows `agent_delivery` like `asides mcp` does. With `--timeout` it stops by itself and
//! prints the command that carries on from there (`--after`), so nothing recorded in between is lost.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use asides_core::agents::{self, Listener, Listening};
use asides_core::config::{AgentDelivery, Config, pretty_shortcut};
use asides_core::paths::{self, tilde};
use asides_core::state::AppState;

use crate::mcp;

const POLL: Duration = Duration::from_millis(400);
/// Checking the app's binding starts a process, so it's checked less often than the notes.
const CHECK_BINDING: Duration = Duration::from_secs(10);

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn stop(_: libc::c_int) {
    STOP.store(true, Ordering::Relaxed);
}

pub fn run(dir: PathBuf, timeout: Option<u64>, after: Option<String>) -> Result<()> {
    // Stopped by the agent (or Ctrl-C): finish the poll, then remove the listener record.
    for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        unsafe { libc::signal(signal, stop as *const () as libc::sighandler_t) };
    }
    let delivery = || Config::load(&paths::config_path()).unwrap_or_default().agent_delivery;
    let sends = || AppState::read(&paths::state_path()).map(|s| s.agent_sends).unwrap_or_default();
    let mut listening = match &after {
        Some(token) => Listening::resume(&dir, token).context("--after takes the token `asides listen` printed")?,
        None => Listening::start(&dir, delivery(), &sends()),
    };
    let record = Listener {
        pid: std::process::id(),
        client: client(),
        dir: dir.clone(),
        waiting: true,
        updated_ms: crate::now_ms(),
    };
    let _ = record.write(&paths::listeners_dir());

    let mode = match delivery() {
        AgentDelivery::Each => "Each aside arrives here as soon as it's transcribed.".to_string(),
        AgentDelivery::Batch => {
            let shortcut =
                pretty_shortcut(&Config::load(&paths::config_path()).unwrap_or_default().shortcuts.send_to_agent);
            format!("Asides arrive here in batches, when the user chooses Send to Agent ({shortcut}).")
        }
    };
    say(&format!("Listening for asides in {}. {mode}", tilde(&dir)));

    let started = Instant::now();
    let mut problem: Option<String> = None;
    let mut next_check = started;
    while !STOP.load(Ordering::Relaxed) {
        if timeout.is_some_and(|secs| started.elapsed() >= Duration::from_secs(secs)) {
            let secs = started.elapsed().as_secs();
            let took = if secs < 60 { format!("{secs} s") } else { format!("{} min", (secs + 30) / 60) };
            let command = command(&dir, timeout, Some(&listening.token()));
            say(&format!("Stopped listening after {took}. To keep listening, run:\n\n{command}"));
            break;
        }
        if Instant::now() >= next_check {
            next_check += CHECK_BINDING;
            let now = mcp::binding_problem(&dir);
            if now != problem {
                match &now {
                    Some(text) => say(&format!("{text} Run `asides` in {} to record asides here.", tilde(&dir))),
                    None if problem.is_some() => say(&format!("Asides is bound to {} again.", tilde(&dir))),
                    None => {}
                }
                problem = now;
            }
        }
        let items = listening.take(delivery(), sends().first());
        if !items.is_empty() {
            say(&mcp::asides_text(&items).0);
        }
        std::thread::sleep(POLL);
    }
    let _ = fs::remove_file(Listener::file(&paths::listeners_dir(), std::process::id()));
    Ok(())
}

/// Writes one event: all of it at once, so a watching agent gets it as one notification.
fn say(text: &str) {
    let mut out = io::stdout().lock();
    let _ = writeln!(out, "{text}\n");
    let _ = out.flush();
}

/// The agent running this, from the variables agents set for the commands they run.
fn client() -> String {
    if std::env::var_os("CLAUDECODE").is_some() {
        return "Claude Code".into();
    }
    agents::client_label(&std::env::var("AI_AGENT").unwrap_or_default())
}

/// The command line for `asides listen` in `dir`: `asides` when that's this program on the PATH,
/// otherwise its full path.
pub fn command(dir: &Path, timeout: Option<u64>, after: Option<&str>) -> String {
    let exe = std::env::current_exe().and_then(fs::canonicalize).ok();
    let on_path = std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|d| fs::canonicalize(d.join("asides")).ok().is_some_and(|p| Some(p) == exe))
    });
    let program = match (&exe, on_path) {
        (Some(exe), false) => quote(&exe.to_string_lossy()),
        _ => "asides".into(),
    };
    let mut command = format!("{program} -C {} listen", quote(&dir.to_string_lossy()));
    if let Some(secs) = timeout {
        command.push_str(&format!(" --timeout {secs}"));
    }
    if let Some(token) = after {
        command.push_str(&format!(" --after {}", quote(token)));
    }
    command
}

/// `text` as one shell word.
fn quote(text: &str) -> String {
    if !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+@%=:,".contains(c)) {
        text.to_string()
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_shell_words() {
        assert_eq!(quote("/Users/sam/code/shop"), "/Users/sam/code/shop");
        assert_eq!(quote("/Users/sam/My Repo"), "'/Users/sam/My Repo'");
        assert_eq!(quote("0:2:14:2026-10-16-0800 Sam's note"), r"'0:2:14:2026-10-16-0800 Sam'\''s note'");
        assert_eq!(quote(""), "''");
    }
}
