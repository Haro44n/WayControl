#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_root="$(mktemp -d)"
cleanup() {
    if [[ -n "${shell_pid:-}" ]]; then kill "$shell_pid" 2>/dev/null || true; fi
    if [[ -n "${weston_pid:-}" ]]; then kill "$weston_pid" 2>/dev/null || true; fi
    rm -rf "$tmp_root"
}
trap cleanup EXIT

export HOME="$tmp_root/home"
export XDG_CONFIG_HOME="$HOME/.config"
export XDG_DATA_HOME="$HOME/.local/share"
export XDG_CACHE_HOME="$HOME/.cache"
export XDG_RUNTIME_DIR="$tmp_root/runtime"
mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME/gnome-shell/extensions" "$XDG_CACHE_HOME" "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"

extension_id=waycontrol@haro44n.github.io
extension_dir="$XDG_DATA_HOME/gnome-shell/extensions/$extension_id"
mkdir -p "$extension_dir"
cp -a "$project_root/packaging/gnome/extension/." "$extension_dir/"
gsettings set org.gnome.shell enabled-extensions "['$extension_id']"

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
if ! cargo run --locked --manifest-path "$project_root/Cargo.toml" -p waycontrol-backend-gnome --example gnome-smoke; then
    cat "$tmp_root/gnome-shell.log"
    exit 1
fi
