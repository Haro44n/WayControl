#!/usr/bin/env bash
set -euo pipefail

runtime_dir="$(mktemp -d)"
sway_pid=""
cleanup() {
    status=$?
    if [[ $status -ne 0 && -f "$runtime_dir/sway.log" ]]; then
        cat "$runtime_dir/sway.log"
    fi
    if [[ -n "$sway_pid" ]]; then
        kill "$sway_pid" 2>/dev/null || true
        wait "$sway_pid" 2>/dev/null || true
    fi
    rm -rf "$runtime_dir"
    exit "$status"
}
trap cleanup EXIT

chmod 700 "$runtime_dir"
export XDG_RUNTIME_DIR="$runtime_dir"
export XDG_SESSION_TYPE=wayland
export XDG_CURRENT_DESKTOP=sway
export WLR_BACKENDS=headless
export WLR_LIBINPUT_NO_DEVICES=1
export WLR_RENDERER=pixman

cat >"$runtime_dir/sway.conf" <<'EOF'
output HEADLESS-1 mode 1280x720
focus_follows_mouse no
EOF

sway --unsupported-gpu --config "$runtime_dir/sway.conf" >"$runtime_dir/sway.log" 2>&1 &
sway_pid=$!

for _ in $(seq 1 100); do
    socket_path="$(find "$runtime_dir" -maxdepth 1 -type s -name 'sway-ipc.*.sock' -print -quit)"
    if [[ -n "$socket_path" ]]; then
        export SWAYSOCK="$socket_path"
        break
    fi
    if ! kill -0 "$sway_pid" 2>/dev/null; then
        cat "$runtime_dir/sway.log"
        exit 1
    fi
    sleep 0.1
done

if [[ -z "${SWAYSOCK:-}" ]]; then
    cat "$runtime_dir/sway.log"
    echo "Sway IPC socket did not become ready" >&2
    exit 1
fi

ready=false
for _ in $(seq 1 100); do
    if swaymsg -t get_outputs >/dev/null 2>&1; then
        ready=true
        break
    fi
    if ! kill -0 "$sway_pid" 2>/dev/null; then
        cat "$runtime_dir/sway.log"
        exit 1
    fi
    sleep 0.1
done
if [[ "$ready" != true ]]; then
    echo "Sway IPC did not become ready" >&2
    exit 1
fi
swaymsg exec 'xterm -title WayControlLiveTest -e sh -c "sleep 300"'
WAYCONTROL_TEST_LIVE_SWAY=1 cargo test -p waycontrol-backend-sway \
    live_sway_supports_workspace_window_monitor_power_and_event_operations -- --ignored --nocapture
