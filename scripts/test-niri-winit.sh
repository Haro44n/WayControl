#!/usr/bin/env bash
set -euo pipefail

runtime_dir="$(mktemp -d)"
niri_pid=""
xvfb_pid=""
foot_pid=""
cleanup() {
    status=$?
    if [[ $status -ne 0 ]]; then
        [[ ! -f "$runtime_dir/niri.log" ]] || cat "$runtime_dir/niri.log"
        [[ ! -f "$runtime_dir/foot.log" ]] || cat "$runtime_dir/foot.log"
        [[ ! -f "$runtime_dir/xvfb.log" ]] || cat "$runtime_dir/xvfb.log"
    fi
    for pid in "$foot_pid" "$niri_pid" "$xvfb_pid"; do
        if [[ -n "$pid" ]]; then
            kill "$pid" 2>/dev/null || true
            wait "$pid" 2>/dev/null || true
        fi
    done
    rm -rf "$runtime_dir"
    exit "$status"
}
trap cleanup EXIT

chmod 700 "$runtime_dir"
export XDG_RUNTIME_DIR="$runtime_dir"
export XDG_SESSION_TYPE=wayland
export XDG_CURRENT_DESKTOP=niri
export DISPLAY=:99
export LIBGL_ALWAYS_SOFTWARE=1
unset WAYLAND_DISPLAY NIRI_SOCKET

cat >"$runtime_dir/niri.kdl" <<'EOF'
workspace "WayControlSource"
workspace "WayControlTarget"
EOF

niri validate --config "$runtime_dir/niri.kdl"
env -u SWAYSOCK \
    XDG_CURRENT_DESKTOP=niri XDG_SESSION_DESKTOP=Hyprland XDG_SESSION_TYPE=wayland \
    HYPRLAND_INSTANCE_SIGNATURE=stale-instance \
    NIRI_SOCKET="$runtime_dir/missing-niri.sock" \
    cargo run --quiet --locked -p waycontrol-server -- inspect \
    | python3 -c 'import json,sys; report=json.load(sys.stdin); assert report["compositor"]["id"] == "niri"; assert not any(report["capabilities"].values()); assert report["state"]["available"] is False'

Xvfb "$DISPLAY" -screen 0 1280x720x24 -nolisten tcp >"$runtime_dir/xvfb.log" 2>&1 &
xvfb_pid=$!
for _ in $(seq 1 100); do
    if DISPLAY="$DISPLAY" xdpyinfo >/dev/null 2>&1; then
        break
    fi
    if ! kill -0 "$xvfb_pid" 2>/dev/null; then
        cat "$runtime_dir/xvfb.log"
        exit 1
    fi
    sleep 0.1
done

niri --config "$runtime_dir/niri.kdl" >"$runtime_dir/niri.log" 2>&1 &
niri_pid=$!
for _ in $(seq 1 200); do
    ipc_socket="$(find "$runtime_dir" -maxdepth 1 -type s -name 'niri.wayland-*.sock' -print -quit)"
    wayland_socket="$(find "$runtime_dir" -maxdepth 1 -type s -name 'wayland-*' -print -quit)"
    if [[ -n "$ipc_socket" && -n "$wayland_socket" ]]; then
        export NIRI_SOCKET="$ipc_socket"
        export WAYLAND_DISPLAY="$(basename "$wayland_socket")"
        break
    fi
    if ! kill -0 "$niri_pid" 2>/dev/null; then
        cat "$runtime_dir/niri.log"
        exit 1
    fi
    sleep 0.1
done
if [[ -z "${NIRI_SOCKET:-}" || -z "${WAYLAND_DISPLAY:-}" ]]; then
    echo "Niri IPC and Wayland sockets did not become ready" >&2
    exit 1
fi

ready=false
for _ in $(seq 1 100); do
    if niri msg --json outputs >/dev/null 2>&1; then
        ready=true
        break
    fi
    if ! kill -0 "$niri_pid" 2>/dev/null; then
        cat "$runtime_dir/niri.log"
        exit 1
    fi
    sleep 0.1
done
if [[ "$ready" != true ]]; then
    echo "Niri IPC did not become ready" >&2
    exit 1
fi

env -u SWAYSOCK \
    XDG_CURRENT_DESKTOP=niri XDG_SESSION_DESKTOP=Hyprland XDG_SESSION_TYPE=wayland \
    HYPRLAND_INSTANCE_SIGNATURE=stale-instance \
    cargo run --quiet --locked -p waycontrol-server -- inspect \
    | python3 -c 'import json,sys; report=json.load(sys.stdin); assert report["compositor"]["id"] == "niri"; assert report["capabilities"]["workspaces"] is True; assert report["capabilities"]["windows"] is True; assert report["state"]["available"] is True'

foot --app-id=WayControlLiveTest --title=WayControlLiveTest \
    >"$runtime_dir/foot.log" 2>&1 &
foot_pid=$!
window_list=""
for _ in $(seq 1 100); do
    window_list="$(niri msg --json windows 2>&1 || true)"
    if [[ "$window_list" == *"WayControlLiveTest"* ]]; then
        break
    fi
    if ! kill -0 "$foot_pid" 2>/dev/null; then
        echo "Wayland test client exited before Niri exposed its window" >&2
        cat "$runtime_dir/foot.log"
        exit 1
    fi
    sleep 0.1
done
if [[ "$window_list" != *"WayControlLiveTest"* ]]; then
    echo "Niri did not expose the Foot test window through its own IPC" >&2
    echo "$window_list"
    cat "$runtime_dir/foot.log"
    exit 1
fi
echo "Niri live window JSON: $window_list"
WAYCONTROL_TEST_LIVE_NIRI=1 cargo test -p waycontrol-backend-niri \
    live_niri_supports_workspace_window_output_event_and_window_actions -- --ignored --nocapture
