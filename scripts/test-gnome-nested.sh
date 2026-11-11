#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ "${WAYCONTROL_GNOME_TEST_IN_SESSION:-0}" != 1 ]]; then
    runner_home="$HOME"
    tmp_root="$(mktemp -d)"
    runtime_dir="$tmp_root/runtime"
    mkdir -p "$tmp_root/home" "$tmp_root/home/.config" "$tmp_root/home/.local/share" "$tmp_root/home/.cache" "$runtime_dir"
    chmod 700 "$runtime_dir"
    if dbus-run-session -- env \
        WAYCONTROL_GNOME_TEST_IN_SESSION=1 \
        WAYCONTROL_GNOME_RUNNER_HOME="$runner_home" \
        WAYCONTROL_GNOME_TMP_ROOT="$tmp_root" \
        HOME="$tmp_root/home" \
        XDG_CONFIG_HOME="$tmp_root/home/.config" \
        XDG_DATA_HOME="$tmp_root/home/.local/share" \
        XDG_CACHE_HOME="$tmp_root/home/.cache" \
        XDG_RUNTIME_DIR="$runtime_dir" \
        "$0"; then
        result=0
    else
        result=$?
    fi
    document_mount="$runtime_dir/doc/by-app"
    if [[ -d "$document_mount" ]] && command -v fusermount3 >/dev/null 2>&1; then
        fusermount3 -uz "$document_mount" 2>/dev/null || true
    fi
    if ! rm -rf "$tmp_root"; then
        echo "GNOME test temporary directory could not be fully removed: $tmp_root" >&2
    fi
    exit "$result"
fi

runner_home="$WAYCONTROL_GNOME_RUNNER_HOME"
tmp_root="$WAYCONTROL_GNOME_TMP_ROOT"
export RUSTUP_HOME="${RUSTUP_HOME:-$runner_home/.rustup}"
export CARGO_HOME="${CARGO_HOME:-$runner_home/.cargo}"
cleanup() {
    if [[ -n "${shell_pid:-}" ]]; then kill "$shell_pid" 2>/dev/null || true; fi
    if [[ -n "${weston_pid:-}" ]]; then kill "$weston_pid" 2>/dev/null || true; fi
    if [[ -n "${xvfb_pid:-}" ]]; then kill "$xvfb_pid" 2>/dev/null || true; fi
}
trap cleanup EXIT

export XDG_CURRENT_DESKTOP=GNOME
export XDG_SESSION_DESKTOP=gnome
mkdir -p "$XDG_DATA_HOME/gnome-shell/extensions"
chmod 700 "$XDG_RUNTIME_DIR"

export DISPLAY=:99
Xvfb "$DISPLAY" -screen 0 1280x720x24 -nolisten tcp >"$tmp_root/xvfb.log" 2>&1 &
xvfb_pid=$!
sleep 0.5
kill -0 "$xvfb_pid" 2>/dev/null || { cat "$tmp_root/xvfb.log"; exit 1; }

extension_id=waycontrol@haro44n.github.io
extension_dir="$XDG_DATA_HOME/gnome-shell/extensions/$extension_id"
mkdir -p "$extension_dir"
cp -a "$project_root/packaging/gnome/extension/." "$extension_dir/"
# dbus-run-session starts before this script creates the isolated HOME. Ensure
# activated settings services use the same temporary XDG paths as this client.
dbus-update-activation-environment --all
gsettings set org.gnome.shell disable-user-extensions false
gsettings set org.gnome.shell enabled-extensions "['$extension_id']"
enabled_extensions="$(gsettings get org.gnome.shell enabled-extensions)"
[[ "$enabled_extensions" == *"'$extension_id'"* ]] || {
    echo "GNOME did not persist the nested extension setting: $enabled_extensions" >&2
    exit 1
}

export WAYLAND_DISPLAY=waycontrol-ci
export XDG_SESSION_TYPE=wayland
weston --backend=headless --socket="$WAYLAND_DISPLAY" --idle-time=0 >"$tmp_root/weston.log" 2>&1 &
weston_pid=$!
for _ in $(seq 1 100); do
    [[ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ]] && break
    kill -0 "$weston_pid" 2>/dev/null || { cat "$tmp_root/weston.log"; exit 1; }
    sleep 0.1
done
[[ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ]] || { cat "$tmp_root/weston.log"; exit 1; }

gnome-shell --nested --wayland >"$tmp_root/gnome-shell.log" 2>&1 &
shell_pid=$!
bridge_ready=0
for _ in $(seq 1 60); do
    version="$(gdbus call --session \
        --dest org.waycontrol.GnomeShell \
        --object-path /org/waycontrol/GnomeShell \
        --method org.waycontrol.GnomeShell1.GetVersion 2>/dev/null || true)"
    if [[ "$version" == *"uint32 1"* ]]; then
        bridge_ready=1
        break
    fi
    kill -0 "$shell_pid" 2>/dev/null || break
    sleep 1
done
if [[ "$bridge_ready" != 1 ]]; then
    cat "$tmp_root/gnome-shell.log"
    echo "GNOME Shell did not load the enabled WayControl extension; configured extensions: $enabled_extensions" >&2
    exit 1
fi

if ! cargo run --locked --manifest-path "$project_root/Cargo.toml" -p waycontrol-backend-gnome --example gnome-smoke; then
    cat "$tmp_root/gnome-shell.log"
    exit 1
fi
