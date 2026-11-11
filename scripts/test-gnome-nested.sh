#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
runner_home="$HOME"
export RUSTUP_HOME="${RUSTUP_HOME:-$runner_home/.rustup}"
export CARGO_HOME="${CARGO_HOME:-$runner_home/.cargo}"
tmp_root="$(mktemp -d)"
cleanup() {
    if [[ -n "${shell_pid:-}" ]]; then kill "$shell_pid" 2>/dev/null || true; fi
    if [[ -n "${weston_pid:-}" ]]; then kill "$weston_pid" 2>/dev/null || true; fi
    if [[ -n "${xvfb_pid:-}" ]]; then kill "$xvfb_pid" 2>/dev/null || true; fi
    rm -rf "$tmp_root"
}
trap cleanup EXIT

export HOME="$tmp_root/home"
export XDG_CONFIG_HOME="$HOME/.config"
export XDG_DATA_HOME="$HOME/.local/share"
export XDG_CACHE_HOME="$HOME/.cache"
export XDG_RUNTIME_DIR="$tmp_root/runtime"
export XDG_CURRENT_DESKTOP=GNOME
export XDG_SESSION_DESKTOP=gnome
mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME/gnome-shell/extensions" "$XDG_CACHE_HOME" "$XDG_RUNTIME_DIR"
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
shell_ready=0
for _ in $(seq 1 60); do
    if gdbus call --session \
        --dest org.gnome.Shell \
        --object-path /org/gnome/Shell \
        --method org.freedesktop.DBus.Peer.Ping >/dev/null 2>&1; then
        shell_ready=1
        break
    fi
    kill -0 "$shell_pid" 2>/dev/null || break
    sleep 1
done
if [[ "$shell_ready" != 1 ]]; then
    cat "$tmp_root/gnome-shell.log"
    echo "Nested GNOME Shell did not acquire its session D-Bus name" >&2
    exit 1
fi

enabled=0
for _ in $(seq 1 60); do
    if gnome-extensions enable "$extension_id" >"$tmp_root/gnome-extensions.log" 2>&1; then
        enabled=1
        break
    fi
    kill -0 "$shell_pid" 2>/dev/null || break
    sleep 1
done
if [[ "$enabled" != 1 ]]; then
    cat "$tmp_root/gnome-extensions.log"
    cat "$tmp_root/gnome-shell.log"
    exit 1
fi
if ! cargo run --locked --manifest-path "$project_root/Cargo.toml" -p waycontrol-backend-gnome --example gnome-smoke; then
    cat "$tmp_root/gnome-shell.log"
    exit 1
fi
