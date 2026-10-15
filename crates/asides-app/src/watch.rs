//! Background polling: Wispr Flow's dictation history and the agents listening through `asides mcp`.
//! Like the config watcher, these threads only report back; the main thread acts.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use asides_core::agents::Listener;
use asides_core::wispr::{Dictation, History};
use chrono::Utc;

const WISPR_POLL: Duration = Duration::from_millis(700);
const LISTENERS_POLL: Duration = Duration::from_secs(2);

/// Reports Wispr Flow dictations finished from now on, until the returned flag is set. Each
/// distinct error is reported once.
pub fn wispr_flow(db: PathBuf, report: impl Fn(Result<Dictation, String>) -> bool + Send + 'static) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    std::thread::Builder::new()
        .name("asides-wispr-flow".into())
        .spawn(move || {
            let mut history = History::new(db, Utc::now());
            let mut last_error: Option<String> = None;
            while !stopped.load(Ordering::Relaxed) {
                match history.poll() {
                    Ok(found) => {
                        last_error = None;
                        for dictation in found {
                            if !report(Ok(dictation)) {
                                return;
                            }
                        }
                    }
                    Err(err) if last_error.as_ref() != Some(&err) => {
                        last_error = Some(err.clone());
                        if !report(Err(err)) {
                            return;
                        }
                    }
                    Err(_) => {}
                }
                std::thread::sleep(WISPR_POLL);
            }
        })
        .expect("spawn Wispr Flow thread");
    stop
}

/// Reports the agents listening through `asides mcp` whenever that changes. Records left behind
/// by sessions that ended without cleaning up are removed.
pub fn listeners(dir: PathBuf, report: impl Fn(Vec<Listener>) -> bool + Send + 'static) {
    std::thread::Builder::new()
        .name("asides-listeners".into())
        .spawn(move || {
            let mut last: Option<Vec<Listener>> = None;
            loop {
                let mut alive = Vec::new();
                for listener in Listener::read_all(&dir) {
                    if is_running(listener.pid) {
                        alive.push(listener);
                    } else {
                        let _ = std::fs::remove_file(Listener::file(&dir, listener.pid));
                    }
                }
                if last.as_ref() != Some(&alive) {
                    last = Some(alive.clone());
                    if !report(alive) {
                        return;
                    }
                }
                std::thread::sleep(LISTENERS_POLL);
            }
        })
        .expect("spawn listeners thread");
}

fn is_running(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else { return false };
    // Signal 0 checks that the process exists; EPERM means it does but belongs to someone else.
    pid > 0
        && (unsafe { libc::kill(pid, 0) } == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM))
}
