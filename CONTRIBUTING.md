# Contributing to explainit

Thanks for helping! Bug reports, fixes, docs and features are all welcome. For anything bigger
than a fix, please open an issue first, so we can agree on the approach before you spend time on
it. Read [ARCHITECTURE.md](ARCHITECTURE.md) before changing the app: it explains how the pieces
fit together and lists the invariants the code relies on.

## Set up

You need macOS 14 or later (Apple silicon recommended), recent stable Rust, the Xcode Command
Line Tools and CMake (`brew install cmake`). Full Xcode isn't required.

```bash
git clone https://github.com/mihaicristianfarcas/explainit && cd explainit
make cert        # once, only if you have no Apple Development or Developer ID certificate
make install     # builds, signs and installs ExplainIt.app and the explainit command
make fixtures    # once: test audio generated with the macOS voices
```

- `make install` signs with the best identity it finds (`scripts/signing-identity.sh`): Developer
  ID Application, then Apple Development, then "ExplainIt Dev". A stable identity is what lets
  macOS keep the microphone permission across rebuilds; with an ad hoc signature it asks after
  every `make install`. `make cert` creates the self-signed "ExplainIt Dev" identity and asks for
  your password.
- `make fixtures` needs the Romanian voice "Ioana": System Settings → Accessibility → Spoken
  Content → System Voice → Manage Voices… The generated files are gitignored, because the macOS
  license doesn't allow redistributing system-voice audio.
- The first recording downloads the speech model (574 MB) to
  `~/Library/Application Support/ExplainIt/models/`.

## Development loop

| You changed | Check it with |
|---|---|
| `explainit-core` | `cargo test -p explainit-core` |
| the CLI | `cargo run -p explainit-cli -- status` (and the other subcommands) |
| the app | `make install`, then `explainit` in a scratch repo and try it for real |

`make install` quits the running ExplainIt and replaces it.

The app's output (`eprintln!`) is discarded when it's launched normally. To see it, quit the app
(`explainit quit`) and run the binary from a terminal:
`~/Applications/ExplainIt.app/Contents/MacOS/ExplainIt`. In that mode macOS attributes the
microphone permission to your terminal app, so it may ask again.

Before you open a pull request:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --features explainit-app/dev-hooks -- -D warnings
make test
make e2e          # if you changed behavior of the CLI or the app
```

CI runs formatting, clippy, the unit tests and the app bundle on every pull request.

## Tests

| Layer | Command | Covers |
|---|---|---|
| Unit | `make test` | notes, metadata, Markdown, git info, links, state, config, resampling, language choice, icon rendering |
| Speech | `make test-speech` | transcription of the fixtures (needs the model) and a real model download with checksum check (190 MB) |
| End-to-end | `make e2e` | installs a build with the `dev-hooks` feature, then drives it through the `explainit` command in a throwaway git repo, feeding WAV files instead of the microphone (`explainit simulate`) |
| Manual | checklist below | everything a script can't do: the microphone, shortcuts, menus and permissions |

To look at the menu-bar icons without installing, render them to files:

```bash
EXPLAINIT_ICON_DUMP=/tmp/icons cargo test -p explainit-app icons
```

This writes raw RGBA data (`<symbol>-<width>x<height>.rgba`) for each icon.

### Manual QA checklist

Run the relevant parts when you change the app's UI, recording, shortcuts or permissions, and say
in your pull request what you tried.

- [ ] **First launch after `tccutil reset Microphone io.github.mihaicristianfarcas.explainit`:**
  the microphone prompt names ExplainIt, not your terminal.
- [ ] **Shortcuts with a browser in front, including full screen:**
  - `⌃⌥Space` tap starts, a second tap stops
  - holding `⌃⌥Space` records until release
  - `⌃⌥N` records into a new note
  - `Esc` cancels and writes nothing
  - Esc still works normally in other apps afterwards
- [ ] **Screenshots:** `⌃⇧⌘4` while recording is placed at the right sentence; `⌃⌥V` while idle
  appends to the note.
- [ ] **Menu:**
  - the icon states: idle, red with a timer, ellipsis, `✓ #N`, warning
  - Directory switching: recent repos and Choose Directory… (focus returns to your previous app)
  - flipping a Preferences switch leaves the menu open and is saved to `config.toml`
- [ ] **Robustness:**
  - a recording started while the previous one is still transcribing
  - AirPods as the microphone
  - sleep while recording
  - the bound directory deleted while the app runs
- [ ] **CLI:** closing the terminal after `explainit` leaves the app running; `explainit quit`
  finishes queued transcriptions first.

## Common changes

- **A setting:**
  1. Add the field to `Config` in `config.rs`, with a default so older `config.toml` files still
     load.
  2. Read it where it's needed in the app.
  3. If it's an on/off setting, add a `Preference` variant in `tray.rs` (`ALL`, `title`, `get`,
     `set`); it gets a switch row in the menu automatically.
  4. Document it in the README's Settings block.
- **A menu item:**
  1. Add a `MenuAction` variant and the item in `tray.rs` (`Tray::new`, `Tray::action`, and
     `show` or `sync_config` if its state changes). Standard items close the menu when clicked;
     use a switch row (`toggles.rs`) for anything the user may want to flip several times.
  2. Handle the action in `App::on_menu`.
  3. Escape `&` in dynamic text with `menu_text`.
- **A CLI command:**
  - Add a `Command` variant in `explainit-cli/src/main.rs`.
  - If it needs the app, add a `Link` variant in `link.rs` (with a round-trip test) and handle it
    in `App::handle_link`.
  - Replies go through `state.json`: add a field to `AppState` with a serde default.
- **A model:** add it to `MODELS` in `models.rs` with the exact size and SHA-256 from the Hugging
  Face file listing. All models must be multilingual.
- **The note format:** change `markdown.rs` or `frontmatter.rs` along with their tests, then the
  e2e expectations and the README example. Keep the format append-only (see the invariants in
  ARCHITECTURE.md).

## Style

- `rustfmt` (120 columns, see `rustfmt.toml`) and clippy with warnings treated as errors.
- Put logic in `explainit-core` when it doesn't need macOS. That's where it can be unit-tested.
- Comments explain why, not what. Match the surrounding code's naming and comment density.
- Error messages are shown to users in the menu and the CLI, so make them actionable: say what
  happened and what to do. Avoid `unwrap()` on anything that depends on user data, files or the
  system.
- Don't add dependencies lightly; each one adds build time to every contributor and to CI.

## Commits and pull requests

- Keep each pull request focused, and each commit building and passing tests on its own.
- Commit subjects use the imperative mood ("Add …", "Fix …") and stay under about 72 characters.
  The body explains why.
- Fill in the pull request template: what changed, why, and how you tested it, including manual QA.

## Reporting bugs

Use the bug report template. Include:
- your macOS version
- `explainit --version`
- the output of `explainit status`
- the steps to reproduce

Notes and audio clips can contain private information, so share only the relevant excerpt.

## License

By contributing, you agree that your contributions are dual licensed under the MIT and Apache-2.0
licenses, as described in the [README](README.md#license).
