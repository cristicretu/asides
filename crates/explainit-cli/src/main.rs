use std::path::{Path, PathBuf};
use std::process::Command as Process;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use explainit_core::config::{Config, pretty_shortcut};
use explainit_core::link::Link;
use explainit_core::notes::{self, Note};
use explainit_core::paths::{self, BUNDLE_ID, tilde};
use explainit_core::project;
use explainit_core::state::AppState;

/// Talk through bugs while you test; ExplainIt appends what you said to a Markdown note in this repo.
///
/// Run `explainit` in a repo (or any directory) to bind the menu-bar app to it, then use the
/// global shortcuts while testing. Notes live in `<repo>/.explainit/`.
#[derive(Parser)]
#[command(name = "explainit", version)]
struct Cli {
    /// Run as if explainit was started in DIR (like `git -C`).
    #[arg(short = 'C', global = true, value_name = "DIR")]
    dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Bind this directory; the next recording starts a new note (titled TITLE, or after your first sentence).
    New { title: Vec<String> },
    /// Print the path of the latest note in this directory.
    Path,
    /// Open the latest note in the default Markdown editor.
    Open,
    /// Show what the menu-bar app is doing.
    Status,
    /// Zip the latest note folder (notes.md + images) for sharing.
    Export {
        /// Where to write the zip (default: ./<note folder>.zip).
        #[arg(long, value_name = "FILE")]
        out: Option<PathBuf>,
    },
    /// Quit the menu-bar app.
    Quit,
    /// Development: process a WAV file as if it had just been recorded.
    #[command(hide = true)]
    Simulate {
        wav: PathBuf,
        #[arg(long)]
        new: bool,
        #[arg(long)]
        image: Option<PathBuf>,
        #[arg(long, default_value_t = 0.0)]
        at: f32,
    },
}

fn main() {
    // Rust ignores SIGPIPE, which turns `explainit status | head -1` into a panic. Behave like other
    // command-line tools instead: exit quietly when the reader goes away.
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
    if let Err(err) = run(Cli::parse()) {
        eprintln!("explainit: {err:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let start = match &cli.dir {
        Some(dir) => dir.clone(),
        None => std::env::current_dir()?,
    };
    let dir = || project::resolve_bind_dir(&start).with_context(|| format!("cannot use {}", start.display()));
    match cli.command {
        None => bind(&dir()?, false, None),
        Some(Command::New { title }) => {
            let title = Some(title.join(" ")).filter(|t| !t.trim().is_empty());
            bind(&dir()?, true, title)
        }
        Some(Command::Path) => {
            println!("{}", latest(&dir()?)?.file().display());
            Ok(())
        }
        Some(Command::Open) => {
            let file = latest(&dir()?)?.file();
            check(Process::new("open").arg(&file), "open the note")
        }
        Some(Command::Status) => status(&dir()?),
        Some(Command::Export { out }) => export(&dir()?, out),
        Some(Command::Quit) => {
            if running_state().is_none() {
                println!("ExplainIt is not running.");
                return Ok(());
            }
            send(&Link::Quit)
        }
        Some(Command::Simulate { wav, new, image, at }) => {
            let request = request_id();
            let wav = wav.canonicalize().context("WAV file not found")?;
            let image = image.map(|i| i.canonicalize()).transpose().context("image not found")?;
            send(&Link::Simulate { wav, new_note: new, image, image_at: at, request: Some(request.clone()) })?;
            wait_for_ack(&request)?;
            println!("queued");
            Ok(())
        }
    }
}

fn bind(dir: &Path, new_note: bool, title: Option<String>) -> Result<()> {
    if project::ensure_git_exclude(dir)? {
        println!("Added .explainit/ to .git/info/exclude (local only; .gitignore untouched).");
    }
    let request = request_id();
    send(&Link::Bind { dir: dir.to_path_buf(), new_note, title: title.clone(), request: Some(request.clone()) })?;
    let state = wait_for_ack(&request)?;

    let config = Config::load(&paths::config_path()).unwrap_or_default();
    let target = if new_note {
        match &title {
            Some(t) => format!("next recording starts a new note \"{t}\""),
            None => "next recording starts a new note (titled after your first sentence)".into(),
        }
    } else {
        match notes::latest_note(dir) {
            Some(note) => format!("appending to \"{}\" ({})", note.title, entries(note.entry_count())),
            None => "first recording creates a note".into(),
        }
    };
    println!("ExplainIt ready in {}", tilde(dir));
    println!("  → {target}");
    println!(
        "  {} record · {} record into a new note · {} attach clipboard image · {} cancel",
        pretty_shortcut(&config.shortcuts.record),
        pretty_shortcut(&config.shortcuts.record_new_note),
        pretty_shortcut(&config.shortcuts.attach_image),
        pretty_shortcut(&config.shortcuts.cancel),
    );
    if state.microphone == "denied" {
        println!("  ! Microphone access is denied: System Settings → Privacy & Security → Microphone → ExplainIt.");
    }
    if (state.status == "downloading" || state.status == "error")
        && let Some(detail) = state.detail
    {
        println!("  ! {detail}");
    }
    Ok(())
}

fn status(dir: &Path) -> Result<()> {
    match running_state() {
        None => println!("ExplainIt is not running. Run `explainit` in a repo to start it."),
        Some(state) => {
            println!("ExplainIt is running (pid {}) · {}", state.pid, state.status);
            if let Some(detail) = &state.detail {
                println!("  {detail}");
            }
            match &state.bound_dir {
                Some(bound) => println!("  bound to {}", tilde(bound)),
                None => println!("  not bound to a directory"),
            }
            if let Some(pending) = &state.pending_new {
                let title = pending.title.as_deref().map(|t| format!(" \"{t}\"")).unwrap_or_default();
                println!("  next recording starts a new note{title}");
            }
            if state.microphone != "granted" && !state.microphone.is_empty() {
                println!("  microphone: {}", state.microphone);
            }
        }
    }
    match notes::latest_note(dir) {
        Some(note) => println!("Latest note here: {} ({})", tilde(&note.file()), entries(note.entry_count())),
        None => println!("No notes in {} yet.", tilde(dir)),
    }
    Ok(())
}

fn export(dir: &Path, out: Option<PathBuf>) -> Result<()> {
    let note = latest(dir)?;
    let name = note.dir.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let out = match out {
        Some(out) => out,
        None => std::env::current_dir()?.join(format!("{name}.zip")),
    };
    notes::export_zip(&note, &out).with_context(|| format!("could not write {}", out.display()))?;
    println!("{}", out.display());
    Ok(())
}

fn latest(dir: &Path) -> Result<Note> {
    notes::latest_note(dir).with_context(|| format!("no notes in {} yet", tilde(dir)))
}

/// Hands the link to the app via Launch Services (launching it if needed, without stealing focus).
fn send(link: &Link) -> Result<()> {
    let out = Process::new("open").args(["-g", "-b", BUNDLE_ID]).arg(link.to_url()).output()?;
    if !out.status.success() {
        bail!(
            "could not reach ExplainIt.app ({}). Is it installed? Run `make install` in the explainit repo.",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

fn wait_for_ack(request: &str) -> Result<AppState> {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if let Some(state) = AppState::read(&paths::state_path())
            && state.last_request.as_deref() == Some(request)
        {
            return Ok(state);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    bail!("ExplainIt did not respond within 15 s")
}

/// The published state, if the app that wrote it is still alive.
fn running_state() -> Option<AppState> {
    let state = AppState::read(&paths::state_path())?;
    let alive = Process::new("kill").args(["-0", &state.pid.to_string()]).output().ok()?.status.success();
    (state.pid != 0 && alive).then_some(state)
}

fn check(cmd: &mut Process, what: &str) -> Result<()> {
    let status = cmd.status().with_context(|| format!("failed to {what}"))?;
    if !status.success() {
        bail!("failed to {what}");
    }
    Ok(())
}

fn request_id() -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    format!("{}-{nanos}", std::process::id())
}

fn entries(n: u32) -> String {
    if n == 1 { "1 entry".into() } else { format!("{n} entries") }
}
