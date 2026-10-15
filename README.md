# explainit

**Talk through bugs while you test. Get them appended, transcribed on your Mac, to a Markdown note
in the repo you're working on**, with screenshots inline, ready to hand to a colleague or a coding agent.

```
$ cd ~/code/shop && explainit
ExplainIt ready in ~/code/shop
  → appending to "Checkout session" (4 entries)
  ⌃⌥Space record · ⌃⌥N record into a new note · ⌃⌥V attach clipboard image · Esc cancel
```

Notice a bug → press `⌃⌥Space` and say what's wrong → press `⌃⌥Space` again → keep testing.
A few seconds later the transcript is in the note, and the menu-bar icon shows `✓ #5`.

- **Local and free.** Speech is transcribed by [whisper.cpp](https://github.com/ggml-org/whisper.cpp)
  on the GPU (Metal). No account, no API key, and no audio leaves your Mac.
- **Verbatim.** What you said, as Whisper heard it. No rewriting, no summarizing.
- **Multilingual.** Any language Whisper supports, detected per recording. You can restrict
  detection to the languages you speak.
- **Screenshots in place.** Copy a screenshot while you talk (`⌃⇧⌘4`) and it lands next to the
  sentence you were saying.
- **Context for whoever reads it.** Each new note records the git remote, branch, commit and
  uncommitted changes, plus your macOS version, browser and display.
- **Native and quiet.** A menu-bar icon and global shortcuts. No Dock icon, no window.

## Requirements

- macOS 14 or later. Developed and tested on macOS 26 with Apple silicon.
- Rust (recent stable), the Xcode Command Line Tools (`xcode-select --install`) and CMake
  (`brew install cmake`). The full Xcode app is not needed.
- About 600 MB of disk space for the speech model, plus 1 GB of RAM while it's loaded. The model
  unloads after 15 idle minutes.

## Install

Releases are signed and notarized by Apple, and run on Apple silicon Macs.

### Homebrew

This repository is also a Homebrew tap:

```bash
brew tap mihaicristianfarcas/explainit https://github.com/mihaicristianfarcas/explainit
brew install --cask explainit
```

It installs `ExplainIt.app` in `/Applications` and links the `explainit` command into Homebrew's
`bin`. `brew upgrade --cask explainit` updates it; the upgrade quits the app, so run `explainit`
again afterwards. `brew uninstall --cask explainit` removes it, and adding `--zap` also deletes
your settings and the speech model.

### Download

Download `ExplainIt-<version>.zip` from the
[Releases page](https://github.com/mihaicristianfarcas/explainit/releases). Unzip it, move
`ExplainIt.app` to `/Applications`, and link the command that ships inside it:

```bash
mkdir -p ~/.local/bin
ln -s /Applications/ExplainIt.app/Contents/Helpers/explainit ~/.local/bin/explainit
```

### From source

```bash
git clone https://github.com/mihaicristianfarcas/explainit && cd explainit
make install   # ExplainIt.app → ~/Applications, plus a link to its CLI in ~/.local/bin
```

With the download or a source build, make sure `~/.local/bin` is on your `PATH`. On first
launch, ExplainIt asks for microphone access and downloads the speech model (574 MB,
checksum-verified) to `~/Library/Application Support/ExplainIt/models/`.

`make install` signs both binaries with the hardened runtime, using the best identity in your
keychain: a Developer ID Application certificate, then an Apple Development one, then a
self-signed "ExplainIt Dev" one. Any of these keeps the microphone permission across updates.
Without one, the app is signed ad hoc: macOS treats every rebuild as a new app and asks for the
microphone again. If you don't have an Apple certificate, run `make cert` once. It creates the
self-signed identity in your login keychain and asks for your password.

`make uninstall` removes the app and the command. Settings and models stay in
`~/Library/Application Support/ExplainIt`.

## Use

### The command

| Command | |
|---|---|
| `explainit` | Bind the repo you're in (its git root, or the current directory if it isn't a repo) and start the menu-bar app. Recordings append to the latest note there. |
| `explainit new [TITLE]` | The next recording starts a new note, titled TITLE or after your first sentence. |
| `explainit path` | Print the latest note's path, e.g. `claude "fix the bugs in $(explainit path)"`. |
| `explainit open` | Open the latest note in your Markdown editor. |
| `explainit status` | Show what the app is doing and where it's bound. |
| `explainit export [--out FILE]` | Zip the latest note (text, screenshots, audio) to share it. |
| `explainit quit` | Quit the app, after it finishes any queued transcriptions. |

`-C <dir>` runs any command as if it were started in `<dir>`.

### Shortcuts

| Shortcut | |
|---|---|
| `⌃⌥Space` | Start or stop recording into the latest note. Tap to toggle, or hold while you talk. |
| `⌃⌥N` | Start or stop recording into a new note. |
| `⌃⌥V` | Attach the image on the clipboard: inline if you're recording, otherwise at the end of the note. |
| `Esc` | Cancel the recording; nothing is written. Only captured while recording. |

Images you copy while recording are attached automatically.

### The menu

The icon shows what's happening: idle, recording (red, with a timer), transcribing, saved
(`✓ #5`) or a problem. Click it to:

- start, stop or cancel recordings, or attach the clipboard image
- switch between recently used repos, or choose another one (**Directory** submenu)
- open, reveal or copy the path of the latest note
- change preferences: hold to talk, auto-attaching images, sounds, keeping audio clips, the speech
  model and the microphone

## Notes

```
<repo>/.explainit/                      ← added to .git/info/exclude (local only)
  2026-10-15-1432 Checkout session/
    notes.md
    images/02-143851.png
    audio/02-143812.m4a                 ← only with "Keep Audio Clips"
```

```markdown
---
title: Checkout session
created: 2026-10-15T14:32:08+03:00
directory: /Users/me/code/shop
git:
  remote: "git@github.com:acme/shop.git"
  branch: feat/checkout
  commit: a1b2c3d
  commit_subject: Fix cart total rounding
  uncommitted_files: 3
environment:
  macos: 26.5.2
  browser: Firefox 143.0
  display: 3024x1964 @2x
transcription: explainit 0.1.0 · whisper large-v3-turbo (local)
---

## 1 · 14:32

The padding on the summary card should be bigger, to compensate for the sticky header.

![14:38:51](images/01-143851.png)

And on mobile the button text wraps.
```

- **Metadata** is written once, when the note is created.
- **Entries** are numbered, so people can refer to "#3".
- **Writes** only ever append, so an editor or agent that has the note open simply reloads it.
- **Sharing:** pass the note's path to an agent working in the repo, or give colleagues the zip
  from `explainit export`.

## Settings

`~/Library/Application Support/ExplainIt/config.toml` (Preferences → Edit Shortcuts & Settings…).
Changes apply as soon as you save.

```toml
hold_to_talk = true          # holding the record shortcut records until release; a tap toggles
auto_attach_images = true    # attach images copied while recording
sounds = true
keep_audio = false           # keep an .m4a of each recording next to the note
model = "large-v3-turbo"     # or "large-v3" (more accurate, slower) or "small" (faster, less accurate)
input_device = ""            # microphone name; empty means the system default
languages = []               # e.g. ["en", "ro"] to choose only between these; [] means any

[shortcuts]
record = "ctrl+alt+Space"
record_new_note = "ctrl+alt+KeyN"
attach_image = "ctrl+alt+KeyV"
cancel = "Escape"
```

If you speak only a few languages, list them in `languages`. Detection on short clips is then much
more reliable; a single entry skips detection altogether.

## Privacy and permissions

- **Audio** stays on your Mac. It's held in memory until it's transcribed, then discarded, unless
  you turn on "Keep Audio Clips".
- **The network** is used for one thing only: downloading the model from Hugging Face the first
  time (or when you switch models).
- **Microphone** access is required.
- **Clipboard:** macOS may ask once whether ExplainIt may paste from other apps, when it reads a
  screenshot from the clipboard. Choose Allow.
- **No other permissions:** Accessibility, Input Monitoring and Screen Recording are not needed.
  The shortcuts use the system hot-key API.

## Troubleshooting

- **macOS asks for the microphone after every update.** The app is signed ad hoc. Run `make cert`
  once, or install an Apple Development or Developer ID certificate (see Install).
- **A shortcut does nothing.** Another app may already use it. Pick another in `config.toml`.
  macOS refuses shortcuts whose only modifier is Option, or Option and Shift.
- **"Nothing heard."** Check Preferences → Microphone. Bluetooth headsets take about a second to
  switch to their microphone; start talking after the start sound.
- **The text came out in the wrong language.** Set `languages` in `config.toml`.
- **Esc doesn't reach other apps while recording.** That's intentional: Esc cancels the recording.
  It's released again as soon as you stop.

## How it works

- **Two parts.** A menu-bar app, `ExplainIt.app`, does the recording and transcription. The
  `explainit` command tells it which repo to use, through `explainit://` links opened with
  `open -g`, and reads the state the app publishes in `state.json`. The app is never started as a
  child of your terminal: it keeps running when the terminal closes, and macOS asks for the
  microphone on behalf of ExplainIt rather than your terminal.
- **Recording.** Audio is captured with CoreAudio (cpal), resampled to 16 kHz, and transcribed in
  one pass after you stop. With large-v3-turbo on an M4 Pro, 18 s of speech takes about 1 s and
  95 s about 4 s, in the background while you keep testing.
- **Whisper over Apple's SpeechAnalyzer.** Apple's on-device recognizer is faster and needs no
  download, but it lacks many languages (Romanian, for one) and it ignored vocabulary hints in our
  tests.
- **No prompt.** Whisper is not given a vocabulary or prompt. A prompt helps it spell jargon, but it
  also changes what you said: in testing it silently dropped a word.
- **One language per recording.** Whisper detects the language once per recording. If you switch
  languages partway through, it can drop the part in the other language.

## Contributing

Contributions are welcome. [CONTRIBUTING.md](CONTRIBUTING.md) covers setup, the development loop,
tests (including a manual QA checklist) and pull requests. [ARCHITECTURE.md](ARCHITECTURE.md)
explains how the CLI, the app and its threads fit together, and the invariants to keep. Coding
agents follow [AGENTS.md](AGENTS.md); Claude Code loads it through `CLAUDE.md`.

```bash
make test     # unit tests
make e2e      # end-to-end through the CLI (run `make fixtures` once first)
```

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT),
at your option. Unless you explicitly state otherwise, any contribution intentionally submitted for
inclusion in this project by you, as defined in the Apache-2.0 license, shall be dual licensed as
above, without any additional terms or conditions.

The Whisper models are released by OpenAI under the MIT license; the ggml conversions are
downloaded from [ggerganov/whisper.cpp on Hugging Face](https://huggingface.co/ggerganov/whisper.cpp).
