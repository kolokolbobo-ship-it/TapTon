#!/bin/sh
# One-time migration from the legacy per-user TapTone cable setup (a
# pipewire-pulse conf.d snippet loading module-null-sink via pactl, plus a
# ~/.asoundrc) to the system-wide configuration shipped by this package
# (/usr/share/pipewire/pipewire.conf.d/90-taptone-cable.conf +
# /etc/asound.conf). Best effort: never aborts package configuration.
set -u
HOME_DIR="${HOME:-$(getent passwd "$(id -u)" | cut -d: -f6)}"

legacy_conf="$HOME_DIR/.config/pipewire/pipewire-pulse.conf.d/10-taptone-cable.conf"
if [ -f "$legacy_conf" ]; then
    rm -f "$legacy_conf"
    echo "taptone: removed legacy $legacy_conf"
fi

asoundrc="$HOME_DIR/.asoundrc"
if [ -f "$asoundrc" ] && grep -q "taptone_route" "$asoundrc"; then
    rm -f "$asoundrc"
    echo "taptone: removed legacy $asoundrc"
fi

# The old pactl-loaded null sink dies with the previous pipewire-pulse; the
# system-wide config recreates it with a low session priority (never default).
pactl list short modules 2>/dev/null \
    | awk '$2 == "module-null-sink" && /TapTone_Route/ {print $1}' \
    | while read -r mid; do pactl unload-module "$mid" 2>/dev/null || true; done

# Restart the user audio stack so the system-wide cable config is loaded.
# Sequentially, with pauses: restarting pipewire, pipewire-pulse and
# wireplumber in one shot races (wireplumber has come up before pipewire was
# ready and then refused to activate new streams — "pending linkable(s) not
# activated"), which once left the whole desktop unlinked.
systemctl --user restart pipewire 2>/dev/null || true
sleep 3
systemctl --user restart pipewire-pulse 2>/dev/null || true
systemctl --user restart wireplumber 2>/dev/null || true

# Wait for the session to come back, then make sure the restart did not elect
# either cable node as a system default: with a partially enumerated device
# set wireplumber has elected the cable the default output/input, which
# silently swallowed all desktop audio (observed in the field).
if command -v pactl >/dev/null 2>&1; then
    i=0
    while [ "$i" -lt 10 ]; do
        pactl info >/dev/null 2>&1 && break
        i=$((i + 1))
        sleep 1
    done
    sleep 2
    def_sink=$(pactl info 2>/dev/null | awk '/^Default Sink:/{print $3}')
    case "$def_sink" in
        *TapTone*)
            alt=$(pactl list short sinks 2>/dev/null \
                | awk '$2 !~ /TapTone/ && $2 ~ /^alsa_output/ {print $2; exit}')
            if [ -n "${alt:-}" ]; then
                pactl set-default-sink "$alt" 2>/dev/null || true
                echo "taptone: default sink was the cable, restored to $alt"
            fi
            ;;
    esac
    def_src=$(pactl info 2>/dev/null | awk '/^Default Source:/{print $3}')
    case "$def_src" in
        *TapTone*)
            alt=$(pactl list short sources 2>/dev/null \
                | awk '$2 !~ /TapTone/ && $2 !~ /\.monitor/ && $2 ~ /^alsa_input/ {print $2; exit}')
            if [ -n "${alt:-}" ]; then
                pactl set-default-source "$alt" 2>/dev/null || true
                echo "taptone: default source was the cable, restored to $alt"
            fi
            ;;
    esac
fi
