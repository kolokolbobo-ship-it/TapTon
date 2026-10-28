# TapTone

Free, open-source soundboard for voice chats, in the spirit of Soundpad: a sound library with
global hotkeys, downloads via `soundpad://` links and a dual output — your headphones hear the
sound, and the voice chat hears it through a second "microphone route". Green-on-black
interface, no audio driver bundled or required, Windows and Linux support.

* License: MIT (see [`LICENSE`](LICENSE))
* Status & history: [`PROGRESS.md`](PROGRESS.md), [`CHANGELOG.md`](CHANGELOG.md)
* Audio routing guide: [`docs/AUDIO_ROUTING.md`](docs/AUDIO_ROUTING.md)

## How it works

* **Local playback** always works out of the box — no virtual device is needed. Sounds are
  decoded once, previewed before use, and mixed with a software mixer (limiter + soft clipper,
  independent volume per sound/route, instant stop, retrigger policies).
* **Voice-chat routing** uses your operating system's own facilities:
  * Windows: a virtual audio cable (e.g. free [VB-CABLE](https://vb-audio.com/Cable/)). TapTone
    plays into `CABLE Input`; Discord picks `CABLE Output` as its microphone. TapTone detects
    installed cables automatically, shows the paired recording side to use, runs a route test
    and warns if no cable is installed. It never downloads or runs installers for you.
  * Linux: PipeWire/PulseAudio virtual sources (see [`docs/AUDIO_ROUTING.md`](docs/AUDIO_ROUTING.md)).
  * TapTone ships **no audio driver and never modifies system audio components**. An older
    experimental capture-APO route was evaluated and removed in this release
    (see `CHANGELOG.md`).
* **Your microphone** (optional): enable "Mix my microphone into the microphone route" to send
  your voice and the soundboard over the one device Discord listens to. The physical mic is
  only opened on request, never opened as an output, and the route's own recording side is
  refused as a voice source (feedback-loop protection).

## Features

- Sound library: add/remove/rename/search/sort sounds, favourites, duplicates detection,
  durations; errors for missing/damaged files stay visible, and the library survives restarts.
- Supported audio formats: **WAV** and **MP3** (via `symphonia`), the same formats the decoder
  uses for playback and preview.
- Global hotkeys: per-sound assignment, re-binding/removal, conflict detection, an explicit
  mic-mute hotkey, messages when a combination cannot be registered, released on exit
  (`global-hotkey` crate; on Linux registration depends on the desktop environment — see
  known limitations).
- Play/stop/volume controls, overlap protection, queue-free "tickets" so Stop always beats
  Play, damaged/audio-file handling that reports instead of panicking.
- Sound downloads from approved `soundpad://`/`taptone://` links: scheme-validated parsing,
  rejects private IPs and non-HTTP(S) targets, size limits, no auto-download without user
  action, pause/cancel, part-file cleanup, files never executed.
- Interface in **Russian or English** (switch in Settings → Application; the whole UI is one
  language at a time, including error messages from the core modules).
- Settings organized into cards, Turkish-green-on-graphite theme, toasts instead of panics,
  keyboard-friendly focus handling.

## Windows: installation

1. Download the release zip (or build from source below) and unpack it anywhere.
2. Run `taptone.exe` (per-user settings live in `%LOCALAPPDATA%\TapTone\TapTone\data`).
3. Optional: install VB-CABLE yourself if you want voice-chat routing, then follow the
   on-screen "How to set up the microphone route" help or
   [`docs/AUDIO_ROUTING.md`](docs/AUDIO_ROUTING.md).

## Linux: installation

Build from source (see below). Distro packages are prepared but not yet published:
- AUR recipe: [`packaging/arch/PKGBUILD`](packaging/arch/PKGBUILD) (needs a maintainer + real
  checksums after the first tag; see [`packaging/README.md`](packaging/README.md))
- Debian packaging: [`packaging/debian`](packaging/debian) (prepared, never built)
- RPM spec: [`packaging/rpm/taptone.spec`](packaging/rpm/taptone.spec) (prepared, never built)

Runtime libraries the Linux build needs: ALSA + desktop GUI libraries
(`libasound2`, `libxkbcommon`, `libxkbcommon-x11` on X11, `wayland` for Wayland sessions;
`Ubuntu`: `libasound2-dev libxkbcommon-dev libxkbcommon-x11-dev libx11-dev libwayland-dev`).

## Building from source

Requirements: Rust 1.80+ (edition 2021, stable toolchain), plus the Linux system packages
listed above on Linux.

```sh
rustup component add rustfmt clippy   # once
cargo fmt --all
cargo test --all-targets
cargo build --release
# Windows: target\release\taptone.exe   Linux: target/release/taptone
```

CI (`.github/workflows/ci.yml`) runs the same checks on Windows and Ubuntu: `cargo fmt
--check`, `cargo test`, `cargo clippy -D warnings`. No CI step needs a real audio device: the
audio tests are written to pass with no playback/recording hardware present.

## Known limitations

- TapTone cannot magically send program audio into the microphone; a virtual device (Windows
  cable, Linux PipeWire loopback) is required for voice-chat routing. Without it TapTone
  says so plainly and playback still works.
- Linux GUI/runtime behaviour of TapTone itself has **not been verified on the target OS**
  yet (development happens on Windows) — reproducible reports and PRs are very welcome.
- Global hotkeys on Linux depend on the desktop environment (global-hotkey uses the same API
  Valve's games use); some Wayland compositors may not allow them — the app reports
  "Global hotkeys are unavailable" in that case rather than pretending.
- `soundpad://` compatibility covers the link subset validated by tests; third-party sites
  using exotic link shapes are not guaranteed to work, and unsupported actions are rejected.
- The "microphone route" test tone is audible to people in your voice chat (that is how
  it verifies the route); run it with nobody listening.

## Report an issue

Provide your OS, audio setup (which cable), the exact wording of the message you saw, and the
steps to reproduce. Bug templates: `.github/ISSUE_TEMPLATE` (fall back to plain text).
