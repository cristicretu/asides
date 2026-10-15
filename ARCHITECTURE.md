# Architecture

How asides fits together, for anyone about to change it. User-facing behavior is in the
[README](README.md); how to set up, test and submit changes is in [CONTRIBUTING.md](CONTRIBUTING.md).

## Bird's-eye view

```
 terminal                        Asides.app (menu bar, one process)
┌───────────┐  open -g -b …   ┌─────────────────────────────────────────────────────────────┐
│ asides │ ──────────────▶ │ main thread: tao event loop                                 │
│   (CLI)   │ asides://…   │   App: bindings, recording, status icon and menu, shortcuts │
│           │                 │        │ Job                          ▲ Outcome            │
│           │ ◀────────────── │        ▼                              │                    │
└───────────┘   state.json    │ worker thread: resample → Whisper → write note files        │
      │                       └─────────────────────────────────────────────────────────────┘
      │ path / open / export                │ appends
      ▼                                      ▼
            <repo>/.asides/<YYYY-MM-DD-HHMM Title>/notes.md, images/, audio/
```

- **The CLI is a remote control.** It works out which directory to bind, then hands the app a link
  through Launch Services, which launches the app if needed. It reads the app's published state to
  confirm and report. Commands that only read notes (`path`, `open`, `export`) don't involve the
  app at all.
- **The app is one long-lived process.** It is menu-bar only (`LSUIElement`, accessory activation
  policy). The main thread owns the UI and recording; a single worker thread does everything slow
  or file-related.

## Code map

```
crates/
  asides-core/     platform-independent, no macOS or UI dependencies, unit-tested
    notes.rs          note folders, latest-note discovery, titles, append, entry numbering, zip export
    frontmatter.rs    YAML metadata (written once per note)
    markdown.rs       entry composition: segments → paragraphs, image placement by timestamp
    project.rs        bind rules (git root or the current directory), git facts, .git/info/exclude
    link.rs           asides:// link format (the CLI → app protocol)
    state.rs          state.json (the app → CLI protocol), recent directories
    config.rs         config.toml schema and defaults, shortcut pretty-printing
    models.rs         Whisper model catalog (file names, sizes, SHA-256)
    resample.rs       windowed-sinc resampler to 16 kHz
    paths.rs          bundle ID, URL scheme, Application Support paths, tilde()
  asides-cli/      shipped inside the bundle as Asides.app/Contents/Helpers/asides
    main.rs           clap subcommands, `open -g -b <bundle id> <link>`, waits for the app to acknowledge
  asides-app/      binary `asides-app`, renamed Asides inside the bundle
    main.rs           App: event loop, UserEvent dispatch, recording state machine, bind(), refresh(), publish()
    tray.rs           status item and menu (MenuAction, View, Directory submenu)
    icons.rs          SF Symbols rendered to RGBA for tray-icon
    hotkeys.rs        global shortcuts (Carbon hot keys); Esc registered only while recording
    audio.rs          cpal microphone capture → mono f32 at the device rate
    worker.rs         Job queue: transcription, note creation, images, audio, append
    stt.rs            ModelManager (download, verify, wait), Transcriber (whisper-rs), language choice
    clipboard.rs      NSPasteboard: images (PNG, TIFF, file URLs), change counter, copy text
    environment.rs    macOS version, frontmost or last-active browser, display (note metadata)
    macos.rs          microphone permission, folder picker, sounds, open/reveal
packaging/Info.plist  bundle ID, LSUIElement, NSMicrophoneUsageDescription, URL scheme
packaging/Asides.entitlements   microphone entitlement (required by the hardened runtime)
packaging/download/   the Cloudflare Worker that serves the website's disk image (static files only)
scripts/              e2e.sh, make-fixtures.sh, signing-identity.sh, make-signing-cert.sh, release.sh,
                      make-dmg.sh, publish-dmg.sh
docs/                 the website (GitHub Pages): index.html, favicon.svg, og.png
Casks/asides.rb       the Homebrew cask; the repository doubles as a tap
```

## Threads and how they talk

| Thread | Owns | Talks back through |
|---|---|---|
| Main (tao event loop) | `App`, the status item and menu, shortcut registration, the cpal `Stream` | n/a |
| cpal callback (CoreAudio) | pushes samples into `Arc<Mutex<Vec<f32>>>` | the shared buffer, read on stop |
| Worker (`worker::spawn`) | the loaded `Transcriber`, **all note file writes** | `UserEvent::Work(Outcome)` |
| Model download (one per model) | the `.part` file | `UserEvent::Model(id, ModelState)`, plus a `Condvar` for the worker |
| Config watcher | polls `config.toml`'s modification time every 2 s | `UserEvent::ConfigFileChanged` |
| global-hotkey, muda | | `UserEvent::HotKey`, `UserEvent::Menu` |

Background threads never touch AppKit objects. They send `UserEvent`s through the
`EventLoopProxy`, and `App::handle` runs them on the main thread.

## Lifecycles

### Binding (`asides` in a repo)

1. The CLI calls `project::resolve_bind_dir` (git root, otherwise the current directory) and
   `ensure_git_exclude`.
2. It sends `asides://bind?dir=…&new=1&title=…&req=<id>` with `open -g -b <bundle id>`.
3. The app gets `Event::Opened`, then `handle_link`, then `App::bind`. That sets the bound
   directory and the pending "new note" request, moves the directory to the front of the recent
   list, and publishes `state.json` with `last_request = <id>`.
4. The CLI polls `state.json` until it sees its `<id>`, then prints the "ready" line.

Choosing a directory from the menu goes through `bind_from_menu`, which applies the same rules
(repo root, git exclude) and then calls `bind`.

### A recording

1. **Start** (`start_recording`, on a shortcut press or a menu click):
   - checks the binding and the microphone permission, and starts cpal
   - snapshots the environment (browser, display) and the clipboard's change count
   - consumes any pending "new note" request
   - registers Esc and plays the start sound
2. **While recording** (`tick`, every 250 ms): updates the timer in the menu bar. With
   auto-attach on, a changed clipboard holding an image becomes a `CapturedImage` with its offset
   from the start.
3. **Stop** (`stop_recording`): stops cpal and unregisters Esc. Recordings under 0.3 s with no
   images are dropped as accidental. Anything else becomes a `Job::Recording`.
4. **Worker** (`process_recording`):
   - resamples to 16 kHz and checks for silence
   - waits for and loads the model (cached; unloaded after 15 idle minutes)
   - transcribes: language detection limited to `config.languages`, then one beam-search pass
   - picks the target note: the latest one, or a new one with metadata
   - saves images and optional audio, composes the entry and appends it
5. **Outcome:** the main thread flashes `✓ #N`, re-reads the latest note and publishes the state.

## Invariants

Break these only on purpose, and update this document when you do.

1. **`asides-core` stays platform-independent.** No AppKit, objc2 or audio crates. Logic that
   can live there should, because that's where it can be unit-tested.
2. **AppKit, tray and menu objects are only touched on the main thread.** Background work reports
   back through `UserEvent`.
3. **Only the worker writes note files**, one job at a time, in the order submitted. That keeps
   entries ordered and numbered correctly even when recordings queue up.
4. **Notes are append-only.** Entries are added with a single append; existing content is never
   rewritten. Editors and agents can keep the file open, and users can edit notes freely.
5. **Metadata is written once,** when a note is created. Entries carry only `## N · HH:MM`.
6. **Transcripts are verbatim.** No prompt, no vocabulary, no post-processing; in testing, a
   prompt silently dropped words. Changes here need a design discussion first.
7. **The CLI ↔ app protocol is `link.rs` plus `state.rs`.** Change both sides together. New
   `state.json` and `config.toml` fields need serde defaults, so files written by older versions
   still load.
8. **The app is launched by Launch Services, never as a child of a terminal.** Otherwise macOS
   attributes the microphone permission to the terminal, and the app dies with the terminal.
9. **Binary names must differ by more than letter case.** On case-insensitive APFS,
   `asides` and `Asides` in `target/release/` are the same file.
10. **Signed with a stable identity and the hardened runtime.** macOS ties the microphone
    permission to the signing identity, so ad hoc signatures re-prompt after every build. Under the
    hardened runtime every capability needs an entitlement (`packaging/Asides.entitlements`).
11. **No new permissions without discussion.** Today Asides needs only the microphone (plus the
    optional clipboard prompt). Features that would require Accessibility, Input Monitoring or
    Screen Recording change what users must trust.

## Persistent state

| Path | Written by | Notes |
|---|---|---|
| `<dir>/.asides/…` | worker | the user's notes; excluded from git locally |
| `~/Library/Application Support/Asides/config.toml` | app (menu), user | reloaded on change; unknown fields ignored, missing ones defaulted |
| `~/Library/Application Support/Asides/state.json` | app | binding, pending new note, status, recent directories, last acknowledged request |
| `~/Library/Application Support/Asides/models/` | download thread | `.part` while downloading, renamed after the SHA-256 check |

## Design decisions

- **whisper.cpp large-v3-turbo (q5_0) via whisper-rs with Metal.** It covers all of Whisper's
  ~99 languages (Romanian and English included), is fast on Apple silicon (18 s of speech in about 1 s on an M4 Pro), and
  `cargo build` compiles it with no separate install. Apple's SpeechAnalyzer was evaluated and
  rejected: it has no Romanian model and ignored vocabulary hints.
- **One pass after stop instead of streaming.** Single-pass latency is a few seconds even for long
  recordings, and it runs while the user keeps testing. Chunked transcription would only be needed
  to handle language switches within one recording; whole-recording detection drops the minority
  language.
- **Language detection limited to a configured set.** Short clips are often misdetected as a
  related language (Romanian as Italian, for example); restricting the candidates fixes that.
- **Links plus a state file instead of a socket.** Launch Services already guarantees a single
  instance and launches the app on demand, and the CLI only needs a one-shot acknowledgement.
- **SF Symbols rendered to bitmaps.** tray-icon takes RGBA; rendering system symbols keeps the
  icons native in light and dark menu bars. tray-icon caps icons at 22 pt, so they are drawn on a
  22 pt canvas at 2×.
- **Shortcuts through Carbon hot keys** (global-hotkey). They need no Accessibility permission,
  and they report key-up, which makes hold-to-talk possible.
