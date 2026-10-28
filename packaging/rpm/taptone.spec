# TapTone RPM spec. First actually-executed build: 2026-10-28 on Linux Mint
# 22.3 (rpmbuild 4.18) against the released TapTon-1.0.0 source archive.
# Enable the test suite with: rpmbuild -bb --define '_with_check 1'

Name:           taptone
Version:        1.0.0
Release:        5%{?dist}
Summary:        Free soundboard for voice chats: sound library, hotkeys, dual audio output
License:        MIT
# TODO(release): replace with the real project URL, e.g.
# URL:            https://github.com/OWNER/taptone
# Source0:        %{url}/archive/refs/tags/v%{version}.tar.gz#/TapTon-%{version}.tar.gz
Source0:        TapTon-%{version}.tar.gz
# Improved desktop entry (adds %u and soundpad:// scheme handlers); the
# upstream tarball still ships the pre-integration variant.
Source1:        taptone.desktop
BuildArch:      x86_64

# Compile-time: ALSA headers (cpal/alsa-sys). X11/Wayland/GL/xkbcommon are
# dlopen()ed at runtime by winit/glutin, so they are required at runtime only.
BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  gcc
BuildRequires:  pkgconfig(alsa)

# Runtime: ALSA (linked), the rest is dlopen()ed by winit/glutin/egui.
# pipewire-alsa provides the ALSA<->PipeWire plugin used by the "taptone_route"
# pcm this package installs to /etc/asound.conf (the virtual microphone cable).
Requires:       alsa-lib
Requires:       pipewire-alsa
Requires:       libxkbcommon
Requires:       libxkbcommon-x11
Requires:       libwayland-client
Requires:       libwayland-cursor
Requires:       mesa-libGL
Requires:       mesa-libEGL

%description
TapTone plays sound effects on two outputs at once: locally (headphones) and
into a "microphone route" that voice chats (Discord, games) use as their
microphone input. Includes a sound library (search, favourites, duplicates
detection), global hotkeys, a green/black interface and soundpad://-style
link downloads. Ships no audio driver: route sounds to voice chats with the
audio server's virtual devices (PipeWire/PulseAudio).

%prep
%autosetup -n TapTon-%{version} -p1

%build
export CARGO_NET_OFFLINE=true
cargo build --release --locked

%install
install -Dm0755 target/release/taptone %{buildroot}%{_bindir}/taptone
install -Dm0644 assets/icon.png %{buildroot}%{_datadir}/icons/hicolor/256x256/apps/taptone.png
install -Dm0644 %{SOURCE1} %{buildroot}%{_datadir}/applications/taptone.desktop
# Virtual microphone cable (system-wide, survives reboots; see docs/AUDIO_ROUTING.md)
install -Dm0644 packaging/pipewire/90-taptone-cable.conf %{buildroot}%{_datadir}/pipewire/pipewire.conf.d/90-taptone-cable.conf
install -Dm0644 packaging/alsa/asound.conf %{buildroot}%{_sysconfdir}/asound.conf
# Fixed /usr/lib/taptone path: the pipewire.conf.d file starts the mic helper
# from there via context.exec (not %_libexecdir, which differs per distro).
install -Dm0755 packaging/linux/migrate-cable.sh %{buildroot}/usr/lib/taptone/migrate-cable.sh
install -Dm0755 packaging/linux/virtual-mic.sh %{buildroot}/usr/lib/taptone/virtual-mic.sh

%check
%if 0%{?_with_check}
cargo test --locked --all-targets
%endif

%post
# Refresh the icon cache so the (updated) application icon shows up
# immediately instead of after the next login.
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q %{_datadir}/icons/hicolor || true
fi
# Refresh the desktop database (soundpad:// / taptone:// scheme handlers).
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database -q %{_datadir}/applications || true
fi

%files
%license LICENSE
%doc README.md CHANGELOG.md
%{_bindir}/taptone
%{_datadir}/icons/hicolor/256x256/apps/taptone.png
%{_datadir}/applications/taptone.desktop
%config(noreplace) %{_sysconfdir}/asound.conf
%{_datadir}/pipewire/pipewire.conf.d/90-taptone-cable.conf
/usr/lib/taptone/migrate-cable.sh
/usr/lib/taptone/virtual-mic.sh

%changelog
* Wed Oct 28 2026 TapTone maintainers - 1.0.0-5
- New application icon: a green equalizer on a dark rounded tile (the old
  placeholder read as random noise). The package refreshes the icon cache
  after install so the new icon shows up immediately.
- User-facing texts no longer assume Windows on Linux: device-list errors,
  the "All devices ..." header, the microphone-route hint and the protocol
  registration message are worded per OS (PipeWire/PulseAudio hints on
  Linux); Windows-only wording is kept for Windows builds.
* Wed Oct 28 2026 TapTone maintainers - 1.0.0-4
- The cable gained its recording side, the named virtual microphone
  "TapTone Microphone (Virtual)" (VB-CABLE "CABLE Output" equivalent): a
  pw-loopback helper (virtual-mic.sh) started by PipeWire via context.exec
  copies the TapTone_Route sink monitor to a source node voice chats select
  as an ordinary microphone. Fixes the capture-side targeting
  (stream.capture.sink = true; without it wireplumber 0.4.x linked the
  loopback to the default microphone) and uses media.class = Audio/Source
  (PipeWire 1.0.x garbles the ports of a /Virtual loopback playback stream).
- Setup help texts in the app describe the new device pair.
* Wed Oct 28 2026 TapTone maintainers - 1.0.0-3
- The package provisions the virtual microphone cable itself: a system-wide
  PipeWire null sink (pipewire.conf.d, node.priority.session = -1000 so it can
  never become the default output) and the ALSA pcm "taptone_route" via
  /etc/asound.conf (new Requires: pipewire-alsa). No manual pactl/~/.asoundrc
  setup anymore; upgrades migrate the per-user setup automatically.
* Wed Oct 28 2026 TapTone maintainers - 1.0.0-2
- Linux routing fix: the microphone route could silently sit on an ALSA alias of the
  desktop default output ("pipewire", "default", "sysdefault:CARD=..."), sending the
  mixed-in voice into the user's own headphones. Device names now resolve to physical
  targets; default aliases are refused for the route (and hidden from its list),
  same-CARD devices count as one destination, PipeWire null sinks are virtual cables.
* Wed Oct 28 2026 TapTone maintainers - 1.0.0-1
- First executed Linux RPM build (from the TapTon-1.0.0 source archive).
- Corrected source/autosetup names, runtime deps (ALSA linked; X11/Wayland/GL
  dlopen()ed) and made the test suite opt-in via --with check.
