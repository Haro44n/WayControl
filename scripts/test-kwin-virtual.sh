#!/usr/bin/env bash
set -euo pipefail

runtime_dir="$(mktemp -d)"
kwin_pid=""
cleanup() {
    status=$?
    if [[ $status -ne 0 ]]; then
        [[ ! -f "$runtime_dir/kwin.log" ]] || cat "$runtime_dir/kwin.log"
    fi
    if [[ -n "$kwin_pid" ]]; then
        kill "$kwin_pid" 2>/dev/null || true
        wait "$kwin_pid" 2>/dev/null || true
    fi
    rm -rf "$runtime_dir"
    exit "$status"
}
trap cleanup EXIT

chmod 700 "$runtime_dir"
export XDG_RUNTIME_DIR="$runtime_dir"
export XDG_CONFIG_HOME="$runtime_dir/config"
export XDG_CACHE_HOME="$runtime_dir/cache"
export XDG_DATA_HOME="$runtime_dir/data"
export XDG_STATE_HOME="$runtime_dir/state"
export LIBGL_ALWAYS_SOFTWARE=1
mkdir -p "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_DATA_HOME" "$XDG_STATE_HOME"

cat >"$XDG_CONFIG_HOME/kwinrc" <<'EOF'
[Desktops]
Id_1=398fd371-57a3-4cc1-95de-2c174e9ed3a1
Id_2=9827dcb8-92b2-46b8-b4d3-3fc8eaab77d2
Name_1=WayControlMain
Name_2=WayControlTarget
Number=2
Rows=1
EOF

kwin_wayland --virtual --no-lockscreen --no-global-shortcuts --no-kactivities \
    >"$runtime_dir/kwin.log" 2>&1 &
kwin_pid=$!

ready=false
for _ in $(seq 1 200); do
    if dbus-send --session --print-reply --dest=org.kde.KWin \
        /VirtualDesktopManager org.freedesktop.DBus.Properties.Get \
        string:org.kde.KWin.VirtualDesktopManager string:current \
        >/dev/null 2>&1; then
        ready=true
        break
    fi
    if ! kill -0 "$kwin_pid" 2>/dev/null; then
        echo "KWin exited before exporting its virtual desktop D-Bus API" >&2
        exit 1
    fi
    sleep 0.1
done
if [[ "$ready" != true ]]; then
    echo "KWin virtual desktop D-Bus API did not become ready" >&2
    exit 1
fi

WAYCONTROL_TEST_LIVE_KWIN=1 cargo test -p waycontrol-backend-kwin \
    live_kwin_lists_switches_and_streams_virtual_desktops -- --ignored --nocapture
