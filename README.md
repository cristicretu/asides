# asides

**Talk through bugs while you test. Get them appended, transcribed on your Mac, to a Markdown note
in the repo you're working on**, with screenshots inline, ready for a colleague, or handed straight to
your coding agent.

```
$ cd ~/code/shop && asides
Asides ready in ~/code/shop
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
- **Straight to your agent.** Claude Code, Codex and OpenCode can listen through `asides mcp`, and
  get each aside as soon as it's transcribed, or a batch when you send it.
- **Wispr Flow too.** If you dictate with Wispr Flow, your dictations can land in the note next to
  your recordings.
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
brew tap mihaicristianfarcas/asides https://github.com/mihaicristianfarcas/asides
brew install --cask mihaicristianfarcas/asides/asides
```

Homebrew 7 and later only load casks from other taps once you trust them, and installing by the
full name does that. After that, the short name `asides` works.

It installs `Asides.app` in `/Applications` and links the `asides` command into Homebrew's
`bin`. `brew upgrade --cask asides` updates it; the upgrade quits the app, so run `asides`
again afterwards. `brew uninstall --cask asides` removes it, and adding `--zap` also deletes
your settings and the speech model.

### Download

Download the [disk image](https://download.asides.mihaicristian.dev/Asides.dmg) (or
`Asides-<version>.zip` from the [Releases page](https://github.com/mihaicristianfarcas/asides/releases)),
drag `Asides.app` to `/Applications`, and open it. The first time, it links the `asides` command
that ships inside it into `/usr/local/bin`, which is on every shell's `PATH`. If that folder
belongs to the system, as it usually does on Apple silicon, macOS asks for your password. Cancel,
and Asides doesn't ask again; you can then link it yourself:

```bash
sudo ln -s /Applications/Asides.app/Contents/Helpers/asides /usr/local/bin/asides
```

### From source

```bash
git clone https://github.com/mihaicristianfarcas/asides && cd asides
make install   # Asides.app → ~/Applications, plus a link to its CLI in ~/.local/bin
```

With a source build, make sure `~/.local/bin` is on your `PATH`. On first launch, Asides asks for microphone access and downloads the speech model (574 MB,
checksum-verified) to `~/Library/Application Support/Asides/models/`.

`make install` signs both binaries with the hardened runtime, using the best identity in your
keychain: a Developer ID Application certificate, then an Apple Development one, then a
self-signed "Asides Dev" one. Any of these keeps the microphone permission across updates.
Without one, the app is signed ad hoc: macOS treats every rebuild as a new app and asks for the
microphone again. If you don't have an Apple certificate, run `make cert` once. It creates the
self-signed identity in your login keychain and asks for your password.

`make uninstall` removes the app and the command; for a download, delete `Asides.app` and
`/usr/local/bin/asides`. Settings and models stay in `~/Library/Application Support/Asides`.

## Use

### The command

| Command | |
|---|---|
| `asides` | Bind the repo you're in (its git root, or the current directory if it isn't a repo) and start the menu-bar app. Recordings append to the latest note there. |
| `asides new [TITLE]` | The next recording starts a new note, titled TITLE or "Untitled". |
| `asides path` | Print the latest note's path, e.g. `claude "fix the bugs in $(asides path)"`. |
| `asides open` | Open the latest note in your Markdown editor. |
| `asides status` | Show what the app is doing and where it's bound. |
| `asides export [--out FILE]` | Zip the latest note (text, screenshots, audio) to share it. |
| `asides send` | Send to Agent: hand what you've recorded to agents listening in batch mode. |
| `asides mcp` | The MCP server for coding agents. Agents start it themselves; see [Coding agents](#coding-agents). |
| `asides listen` | Print your asides as they reach agents. Claude Code runs it in the background while it listens. |
| `asides quit` | Quit the app, after it finishes any queued transcriptions. |

`-C <dir>` runs any command as if it were started in `<dir>`.

### Shortcuts

| Shortcut | |
|---|---|
| `⌃⌥Space` | Start or stop recording into the latest note. Tap to toggle, or hold while you talk. |
| `⌃⌥N` | Start or stop recording into a new note. |
| `⌃⌥V` | Attach the image on the clipboard: inline if you're recording, otherwise at the end of the note. |
| `Esc` | Cancel the recording; nothing is written. Only captured while recording. |
| `⌃⌥Enter` | Send to Agent. Only registered while agents get asides in batches. |

Images you copy while recording are attached automatically. You can change any shortcut in
**Preferences → Edit Shortcuts & Settings** (see [Settings](#settings)).

### The menu

The icon shows what's happening: idle, recording (red, with a timer), transcribing, saved
(`✓ #5`) or a problem. Click it to:

- start, stop or cancel recordings, or attach the clipboard image
- send what you've recorded to your coding agent (in batch mode)
- switch between recently used repos, or choose another one (**Directory** submenu)
- open, reveal or copy the path of the latest note
- change preferences: hold to talk, auto-attaching images, sounds, keeping audio clips, Wispr Flow
  dictations, the speech model, the microphone, and how agents get asides

The status line shows when an agent is listening, e.g. `Ready · shop · Claude Code listening`.

## Notes

```
<repo>/.asides/                      ← added to .git/info/exclude (local only)
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
transcription: asides 0.1.0 · whisper large-v3-turbo (local)
---

## 1 · 14:32

The padding on the summary card should be bigger, to compensate for the sticky header.

![14:38:51](images/01-143851.png)

And on mobile the button text wraps.
```

- **Metadata** is written once, when the note is created.
- **Entries** are numbered, so people can refer to "#3".
- **Writes** only ever append, so an editor or agent that has the note open simply reloads it.
- **Sharing:** let your coding agent listen (below), pass it the note's path, or give colleagues the
  zip from `asides export`.

## Coding agents

`asides mcp` is an [MCP](https://modelcontextprotocol.io) server. Add it to your agent once:

```bash
claude mcp add --scope user asides -- asides mcp      # Claude Code
codex mcp add asides -- asides mcp                    # Codex
opencode mcp add --global asides -- asides mcp        # OpenCode
```

Then, in a session in your repo, ask the agent to **listen to your asides** (in Claude Code,
`/asides:listen` says the same). It acts on each aside as a prompt, until you tell it to stop. If
Asides is bound to another repo, or not running, the agent binds it to its own (it may ask you to
allow that once).

How the agent listens depends on what it can do:

- **Claude Code** runs `asides listen` in the background, with its Monitor tool. Each aside shows
  up in the chat as it reaches the agent, and the agent acts on it. In between, the agent is free:
  you can ask it other things.
- **Codex, OpenCode and others** wait in a tool call (`wait_for_asides`), act on what it returns,
  then wait again. Asides that arrive while the agent works wait for it. Most agents hide tool
  results, so the agent first lists the asides it got, one line each.

How asides reach the agent is a preference (**Preferences → Agents Get Asides**, or
`agent_delivery`):

- **Each aside as it's transcribed** (`each`, the default): every recording made after the agent
  starts listening becomes a prompt a few seconds after you stop talking.
- **In batches** (`batch`): recordings wait until you choose **Send to Agent** (`⌃⌥Enter`, the menu,
  or `asides send`). A batch holds everything recorded since the last one, including recordings
  made before the agent started listening. A recording in progress is stopped and included.

The menu shows where things stand. Its status line says `Claude Code listening` while the agent
waits for asides, and `Claude Code working on your asides` while it acts on them. Send to Agent
shows **Sent** when an agent is waiting, and **Queued** when it's busy: it gets the batch when it's
done. With no agent listening, nothing is sent; the menu says so, and the batch waits for your next
Send to Agent.

The agent gets the transcript, the note's path and your screenshots (as images through
`wait_for_asides`, as file paths through both). It can also read the whole latest note on request
("work through my asides", or `/asides:asides` in Claude Code).

Agents give up on a tool call after a while: Codex and most others after 60 s, Claude Code much
later. So the agent's wait ends after 50 s (25 min in Claude Code), and the agent calls again. To
wait longer in Codex, raise its `tool_timeout_sec` for the server and pass the same wait to asides:
`codex mcp add asides -- asides mcp --timeout 3000`, then set `tool_timeout_sec = 3600` under
`[mcp_servers.asides]` in `~/.codex/config.toml`. Claude Code's monitors last at most 30 minutes,
so `asides listen` stops after 29 and prints the command that carries on where it stopped; the
agent runs it, and nothing recorded in between is lost.

## Wispr Flow

If you dictate with [Wispr Flow](https://wisprflow.ai), turn on **Preferences → Add Wispr Flow
Dictations** (`wispr_flow = true`). Each dictation you make while it's on becomes an entry in the
latest note, marked with its source:

```markdown
## 6 · 14:41 · Wispr Flow

The coupon field accepts expired codes.
```

- Asides reads Wispr Flow's history on your Mac (`~/Library/Application Support/Wispr Flow/flow.sqlite`),
  read-only, with the system's `sqlite3`. Wispr Flow must keep history on this Mac.
- Only dictations made after you turn it on are added. List apps in `wispr_flow_apps` (for example
  `["Safari", "Google Chrome"]`) to add only what you dictate there, so your messages elsewhere stay
  out of the note.
- The text is what Wispr Flow typed, formatting included; Asides doesn't change it. Wispr Flow
  transcribes in its own way, not on your Mac with Whisper.
- To attach a screenshot, press `⌃⌥V` after the dictation.

## Settings

`~/Library/Application Support/Asides/config.toml` (Preferences → Edit Shortcuts & Settings…).
Changes apply as soon as you save.

```toml
hold_to_talk = true          # holding the record shortcut records until release; a tap toggles
auto_attach_images = true    # attach images copied while recording
sounds = true
keep_audio = false           # keep an .m4a of each recording next to the note
model = "large-v3-turbo"     # or "large-v3" (more accurate, slower) or "small" (faster, less accurate)
input_device = ""            # microphone name; empty means the system default
languages = []               # e.g. ["en", "ro"] to choose only between these; [] means any
agent_delivery = "each"      # or "batch": agents get asides when you choose Send to Agent
wispr_flow = false           # add Wispr Flow dictations to the note
wispr_flow_apps = []         # e.g. ["Safari"] to add only dictations made there; [] means any app

[shortcuts]
record = "ctrl+alt+Space"
record_new_note = "ctrl+alt+KeyN"
attach_image = "ctrl+alt+KeyV"
cancel = "Escape"
send_to_agent = "ctrl+alt+Enter"   # only registered with agent_delivery = "batch"
```

An empty shortcut (`""`) turns that shortcut off.

If you speak only a few languages, list them in `languages`. Detection on short clips is then much
more reliable; a single entry skips detection altogether.

## Privacy and permissions

- **Audio** stays on your Mac. It's held in memory until it's transcribed, then discarded, unless
  you turn on "Keep Audio Clips".
- **The network** is used by Asides for one thing only: downloading the model from Hugging Face the
  first time (or when you switch models).
- **Agents** talk to `asides mcp` on your Mac, over standard input and output. It reads your notes
  and passes them to the agent, which may send them on to its model provider, like anything else you
  give it.
- **Wispr Flow** transcribes with its own service, under its own terms. Asides only reads the
  history Wispr Flow keeps on your Mac, and only when you turn the integration on.
- **Microphone** access is required.
- **Administrator password:** asked once, if you installed Asides by hand and `/usr/local/bin`
  belongs to the system, to link the `asides` command there. Cancel to skip it.
- **Clipboard:** macOS may ask once whether Asides may paste from other apps, when it reads a
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
- **The agent doesn't get my asides.** Check the status line in the menu: it says which agent is
  listening, or working on your last asides. If none is, ask the agent to listen to your asides. In
  batch mode, choose Send to Agent; it says whether the batch was sent, queued, or had no agent to
  go to. If the agent works in another repo than the one Asides is bound to, recordings go to the
  bound one.
- **Wispr Flow dictations don't show up.** Check that Wispr Flow keeps history on this Mac, and that
  the app you dictate in matches `wispr_flow_apps`.

## How it works

- **Two parts.** A menu-bar app, `Asides.app`, does the recording and transcription. The
  `asides` command tells it which repo to use, through `asides://` links opened with
  `open -g`, and reads the state the app publishes in `state.json`. The app is never started as a
  child of your terminal: it keeps running when the terminal closes, and macOS asks for the
  microphone on behalf of Asides rather than your terminal.
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
