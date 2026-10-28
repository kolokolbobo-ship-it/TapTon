# Packaging

This directory holds the distribution recipes. **Honest status first:**

| Artifact | Prepared | Built | Verified on the target OS |
|---|---|---|---|
| Windows `taptone.exe` (cargo release build) | yes | yes (this machine, ci.yml mirrors it) | yes (runs, audio routes verified) |
| Arch/AUR PKGBUILD | yes | yes (`taptone-1.0.0-3-x86_64.pkg.tar.zst`, 2026-10-28, staged manually; prefer `makepkg` from `PKGBUILD`) | no (build host runs Mint 22.3; manual staging, not `makepkg`) |
| Debian package (dpkg) | yes | yes (`taptone_1.0.0-3_amd64.deb`, 2026-10-28, built on Mint 22.3 / dpkg-deb) | yes (installed, launched and audio-routed on Mint 22.3 — the dev host is the target distro) |
| RPM spec | yes | yes (`taptone-1.0.0-3.x86_64.rpm`, 2026-10-28, rpmbuild 4.18; test suite opt-in via `--define '_with_check 1'`) | no (not run on Fedora/mock yet) |
| AppImage | no (decision below) | no | no |

Note (2026-10-28, first executed Linux builds): the app was built with Rust
1.99.1 on Linux Mint 22.3 (Ubuntu 24.04 base); `cargo test --locked
--all-targets` passes on Linux, the binary opens the ALSA output and creates
its per-user data dirs (`$XDG_DATA_HOME/taptone`). Runtime deps: ALSA is
linked; X11/Wayland/GL/xkbcommon are dlopen()ed, so they are declared as
runtime dependencies in every package. The desktop entry was improved
(`Exec=taptone %u`, `MimeType=x-scheme-handler/soundpad;x-scheme-handler/taptone`);
the tarball still carries the old entry, so the deb/rpm/Arch recipes install
the improved one as an extra source file until the next source release.
Built artifacts live in `dist/` next to this README's project root.

Note (2026-10-28, revision -3): the Linux packages now provision the virtual
microphone cable themselves, the Soundpad-style virtual mic done the Linux way —
with what the audio server already provides, no kernel/driver blobs:

* `/usr/share/pipewire/pipewire.conf.d/90-taptone-cable.conf` creates the
  null sink `TapTone_Route` natively at PipeWire start, with
  `node.priority.session = -1000` so it can **never** be elected as the
  system's default output (a default flip would silently swallow everything
  played to the desktop default — observed and fixed in the field);
* `/etc/asound.conf` exposes it to ALSA apps as the `taptone_route` pcm
  (needs `pipewire-alsa`, now a hard runtime dependency of every package);
* on upgrade, `packaging/linux/migrate-cable.sh` removes the legacy per-user
  setup (the pactl-based pipewire-pulse snippet and `~/.asoundrc`) and
  restarts the user audio stack once.

Note (2026-10-28, revision -4): the cable gained its recording side — the
named virtual microphone "TapTone Microphone (Virtual)". Two PipeWire
behaviour quirks shaped the design (both hit in the field):

* PipeWire processes `context.modules` before `context.objects`, so a
  config-level module-loopback starts before the sink exists and wireplumber
  then links its capture stream to the system default device — which once
  wired the user's real microphone into the cable. The sink is therefore
  created via `context.objects`, and the loopback runs from `context.exec`
  (after the whole config is loaded) via `/usr/lib/taptone/virtual-mic.sh`,
  a child of the pipewire service that dies with its cgroup.
* wireplumber 0.4.x resolves a capture stream's `node.target` to a sink only
  when the stream sets `stream.capture.sink = true`; without it the loopback
  silently records the default microphone instead of the cable.
* Additionally, on PipeWire 1.0.x a loopback playback stream with
  `media.class = Audio/Source/Virtual` gets broken ports (one FR-only input
  port, no capture ports); a plain `Audio/Source` yields proper stereo
  capture ports, so the helper uses that and keeps "(Virtual)" in the
  description.

Note (2026-10-28, revision -5): presentation pass, no behaviour changes.

* new application icon: a green five-bar equalizer on a dark rounded tile
  (`assets/icon.png`, 256x256 — the old placeholder read as random noise).
  The deb `postinst` and the rpm `%post` scriptlet now run
  `gtk-update-icon-cache -q /usr/share/icons/hicolor` after install so the
  new icon shows up immediately (pacman refreshes the cache via its own
  hooks);
* user-facing texts are OS-aware instead of assuming Windows: device-list
  errors, the "All devices ..." header, the microphone-route hint and the
  protocol registration message use system/PipeWire/PulseAudio wording on
  Linux; Windows-only wording is kept for the Windows build (branched at the
  call sites via `std::env::consts::OS`, same pattern as the -4 setup help).

Development of TapTone happens on Windows; until a Linux machine runs `makepkg`/`dpkg-buildpackage`
/`rpmbuild` against these recipes, none of the Linux packages may be advertised as "working"
(every non-recipe file also states this). Steps to change that are listed per directory.

## Arch / AUR (`arch/PKGBUILD`)

`makepkg` is not available in the development environment, so the checksums and the `.SRCINFO`
have **not** been generated — that is expected and normal only after a release exists.
Before publishing to the AUR:

1. Push this repository to GitHub (owner decides the account/URL) and create tag `v0.1.0`
   with the sources matching this tree.
2. Fill in `source`, `url` placeholders (`GITHUB_OWNER`) in the PKGBUILD.
3. Generate checksums: `updpkgsums` (or `makepkg -g`), then `makepkg -f` and try the binary.
4. `makepkg --printsrcinfo > .SRCINFO` and commit both files to the AUR repository with an
   AUR account; choose the package name after checking namespace conflicts
   (`https://aur.archlinux.org/packages?K=taptone`).
5. `namcap PKGBUILD *.pac*` when available.

The PKGBUILD builds from the released source tarball with `cargo` and installs the binary, the
license, an icon and a desktop entry. It contains **no** driver steps, needs no root at build
time and works with the stock `extra/rust` toolchain.

## Debian (`debian/`)

`packaging/debian/` contains `control`, `rules` and install fragments for a source-based .deb
(`dpkg-buildpackage -us -uc -b`). Move the `debian` directory into the repository root of the
released source tree, install build deps (`cargo`, `rustc`, `libasound2-dev`,
`libxkbcommon-dev`, `libxkbcommon-x11-dev`, `libx11-dev`, `libwayland-dev`) and run
`dpkg-buildpackage`. The package does not remove user data on uninstall (all data lives in
`$XDG_DATA_HOME/taptone` or `~/.local/share/...`). Not run yet: see the Makefile `sbuild`
needs differ between Debian versions; the maintainer should verify on a current testing/sid
chroot first.

## RPM (`rpm/taptone.spec`)

A straightforward `%{cargo_build}`-style spec for Fedora. Never executed; treat as a sketch
until someone runs it in mock/koji.

## AppImage — decision: not provided yet

An AppImage would bundle GUI/audio libraries and needs `appimagetool` plus a Linux machine to
build and test runtimes (X11/Wayland, PipeWire resonance). Shipping an untested universal
format would violate the project's honesty rule, so this directory deliberately contains
no AppImage yet. If needed later, build the app in an Ubuntu container, layout an AppDir with
`usr/bin/taptone`, `usr/share/applications/`, `usr/share/icons/`, run again in the target
desktop environment, and document the result here.
