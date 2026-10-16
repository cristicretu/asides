# AGENTS.md

Instructions for coding agents working in this repository. Read [ARCHITECTURE.md](ARCHITECTURE.md)
before changing the app. [CONTRIBUTING.md](CONTRIBUTING.md) has the full human workflow.

## The project

asides is a macOS menu-bar app plus a CLI.
- `asides` binds a repo.
- A global shortcut records speech.
- whisper.cpp transcribes it locally, verbatim.
- The transcript is appended to `<repo>/.asides/<YYYY-MM-DD-HHMM Title>/notes.md`.
- Coding agents can listen through `asides mcp`, an MCP server in the CLI, and get each new
  entry (or a batch, on Send to Agent) as a prompt.
- With the Wispr Flow integration on, Wispr Flow dictations become entries too.

It's a Rust workspace:
- `crates/asides-core`: platform-independent logic.
- `crates/asides-cli`: the `asides` binary.
- `crates/asides-app`: the menu-bar app (binary `asides-app`, bundled as `Asides.app`).

## Commands

| Task | Command |
|---|---|
| Format | `cargo fmt --all` |
| Lint (must be clean) | `cargo clippy --workspace --all-targets --features asides-app/dev-hooks -- -D warnings` |
| Unit tests | `make test` (= `cargo test --workspace`) |
| End-to-end | `make e2e` (needs fixtures and the model; see below) |
| Speech tests | `make test-speech` (needs the model; also downloads 190 MB) |
| Install | `make install` |
| Test fixtures | `make fixtures` (needs the macOS voice "Ioana") |
| Share image | `python3 scripts/make-og.py` (renders `docs/og.png` from the demo; needs Firefox) |
| App icon | `swift scripts/make-icon.swift` (writes `packaging/Asides.icns`) |
| Notarized release | `make release` (only when the maintainer asks; see Rules) |
| Deploy the website | `make publish-site` (only when the maintainer asks) |

Run format, lint and unit tests after every change. Run `make e2e` when CLI or app behavior
changes.

## Rules

- **Follow the invariants in ARCHITECTURE.md.** The ones most often at risk:
  - `asides-core` has no macOS dependencies
  - AppKit, tray and menu code runs on the main thread only
  - only the worker thread writes notes
  - notes are append-only
  - transcripts are verbatim: no prompts, no post-processing
  - new `config.toml` and `state.json` fields get serde defaults
- **Never commit** `.asides/` (the user's notes and voice recordings), `tests/fixtures/`
  (generated with macOS voices, which may not be redistributed), `target/` or `dist/`. They're
  gitignored; keep them that way.
- **Never edit the user's notes** in `.asides/`.
- **Run `make release` only when the maintainer asks.** It uploads the build to Apple's notary
  service and needs their stored notarytool credentials.
- **Don't run `make cert`** unless asked. It changes the login keychain and prompts for a password.
- **Keep the stable signing.** `make bundle` picks the identity with `scripts/signing-identity.sh`
  and signs with the hardened runtime. Don't fall back to ad hoc (the microphone permission would
  reset every build), and add an entitlement for any new capability.
- **Check `asides status` before `make install` or `make e2e`.** If it says recording or
  transcribing, the user is in the middle of something: don't reinstall.
- **Tell the user what `make install` and `make e2e` did.** Both quit the running Asides and
  replace the installed app, and `make e2e` installs a test build with dev hooks. Afterwards run
  `make install` to restore a normal build, and tell the user to run `asides` again.
- **Know what `make e2e` uses.** It relies on the model in
  `~/Library/Application Support/Asides/models/`; if it's missing, the app downloads 574 MB.
  It also rebinds the app to a temporary repo.
- **Keep binary names unique ignoring case** (case-insensitive APFS). Don't name a binary
  `Asides`.
- **Don't add permissions** (Accessibility, Input Monitoring, Screen Recording) without the
  maintainer's agreement.
- **Keep the docs in sync.** Update the README (user-facing), ARCHITECTURE.md (structure,
  invariants) and CONTRIBUTING.md (workflow) when your change affects them. The website
  (`docs/index.html`) repeats the shortcuts, requirements and install steps.

## What you can and can't verify

**You can:**
- run all the automated tests
- drive the installed app through the CLI with `asides simulate <wav> [--new] [--image FILE --at SECONDS]` (dev-hooks builds only; `make e2e` sets this up)
- render the menu-bar icons to inspect them: `ASIDES_ICON_DUMP=<dir> cargo test -p asides-app icons` writes RGBA dumps you can convert to PNG
- read `~/Library/Application Support/Asides/state.json`
- drive `asides mcp` with JSON-RPC on stdin (see CONTRIBUTING.md), and run Claude Code, Codex or
  OpenCode against it with a one-off MCP config, using `asides simulate` for speech
- run `asides listen --timeout N` and read what it prints
- render the website with headless Firefox (`--screenshot`) to check layouts at phone width

**You can't:**
- press global shortcuts
- click the status item or menus
- speak into the microphone
- grant permissions

When a change touches those, finish by listing the exact manual checks for the user to run. The
manual QA checklist in CONTRIBUTING.md has the full list.

## Working from asides notes

The user may record a request and ask you to "implement the note". Find it with
`asides path` (or `asides -C <repo> path`): it's the latest `notes.md` under `.asides/`.

- Entries (`## N · HH:MM`) are raw Whisper output, in English or Romanian. Expect misheard words,
  especially project names (for example "Local Tatus" for "local-tts").
- Early entries may be microphone tests. Act on the entries that ask for something, and say which
  ones you implemented.
- Images referenced in an entry (`images/…`) are screenshots the user attached at that point.
  Look at them.
- The front matter records the branch and commit the user was testing.
- Also report defects the note itself reveals, such as wrong metadata or a failed audio conversion.

## Pitfalls already hit

- `target/release/asides` and `target/release/Asides` are the same file on case-insensitive
  APFS. The bundle once shipped the CLI as the app.
- `afconvert` rejects AAC above 32 kbps for 16 kHz mono (`-b 32000` works).
- `git rev-parse --abbrev-ref HEAD` fails before the first commit; use `git branch --show-current`.
- `ditto -c -k` zips don't flag UTF-8 names, so diacritics get garbled. Export uses the `zip` crate.
- muda drops a single `&` from menu titles (mnemonics); escape dynamic text with `menu_text`.
- tray-icon scales icons to at most 22 pt tall; `icons.rs` draws on a 22 pt canvas at 2×.
- `if let` keeps its scrutinee's temporaries (including a `MutexGuard`) alive for the whole block,
  so locking the same mutex inside it deadlocks. Bind the value first.
- An `Esc` key equivalent on a menu item would cancel the recording when Esc closes the menu, so
  the shortcut is shown in the item's label instead.
- `say -o x.aiff --data-format=LEI16@16000` fails; use `--file-format=WAVE`.
- tao activates the app at launch unless `set_activate_ignoring_other_apps(false)`; a menu-bar app
  must not take focus.
- Changing the status item's icon or title, or a menu item's text, while the menu is open does
  **not** close it (tested). Look elsewhere if the menu closes unexpectedly: focus or activation
  changes, or system prompts.
- Clicking a standard menu item always closes the menu. Switch rows (view-based items) stay open,
  but they were tried for the preferences and reverted: they don't line up with native items.
- Without a `TrayIconEvent` handler, tray-icon queues an event for every mouse move over the icon,
  forever.
- Apple's SpeechAnalyzer and FoundationModels are Swift-only, so Rust can't call them without a
  Swift shim.
- Codex (and clients built on the MCP SDKs' defaults) abandon a tool call after 60 s, so
  `wait_for_asides` returns within 50 s unless the client is Claude Code. Claude Code allows hours,
  but cuts off a stdio call that sends nothing for 30 min; progress notifications keep it alive.
- Codex asks for approval before each call to an MCP tool not marked `readOnlyHint` (and fails
  it under `approval: never`), so a listening loop needs read-only tools. That's why binding is a
  separate tool, `bind_asides`.
- Claude Code's Monitor tool kills a command after at most 30 min, so the listen prompt runs
  `asides listen --timeout 1740`, which prints the command to carry on (with an `--after` token)
  before that happens. Monitor events aren't replies from the user; the prompt says asides are.
- Claude Code probes `server/discover` (a newer MCP revision) before `initialize`. Answering
  "method not found" makes it fall back to `initialize`.
- Wispr Flow's `flow.sqlite` schema changes often between its versions. Discover columns, don't
  assume them, and don't open it any way but read-only: Wispr Flow writes to it in WAL mode.
- On the website, a `padding` shorthand on `.wrap` inside a media query reset the sections' top
  padding on phones. Set only `padding-inline` there.
- Headless Firefox takes `--screenshot` at load, mid-transition. Turn transitions off to capture a
  frame (as `scripts/make-og.py` does).

## Commits

Imperative subjects ("Add …", "Fix …"), a body that explains why, and each commit building and
passing tests on its own. Commit only when the user asks.
