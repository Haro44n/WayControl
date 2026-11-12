#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
daemon="${WAYCONTROL_DAEMON:-$project_root/target/release/waycontrold}"
if [[ ! -x "$daemon" ]]; then
    echo "build the daemon first or set WAYCONTROL_DAEMON to its executable" >&2
    exit 1
fi
daemon="$(realpath "$daemon")"

for dependency in unshare ip python3; do
    command -v "$dependency" >/dev/null || {
        echo "missing required command: $dependency" >&2
        exit 1
    }
done
unshare --user --map-root-user --net -- bash -s -- "$daemon" <<'NAMESPACE'
set -euo pipefail

daemon="$1"
ip link set lo up
ip address add 10.42.0.1/24 dev lo
ip address add 10.42.0.2/24 dev lo
ip address add 10.42.0.3/24 dev lo
ip address add 8.8.8.8/32 dev lo

tmp="$(mktemp -d /tmp/waycontrol-lan.XXXXXX)"
mkdir -p "$tmp/config" "$tmp/runtime"
chmod 700 "$tmp/runtime"
XDG_CONFIG_HOME="$tmp/config" \
XDG_RUNTIME_DIR="$tmp/runtime" \
HOME="$tmp" \
WAYCONTROL_BIND_ADDR=10.42.0.1:18765 \
env -u WAYLAND_DISPLAY -u HYPRLAND_INSTANCE_SIGNATURE \
    "$daemon" serve >"$tmp/server.log" 2>&1 &
daemon_pid=$!

cleanup() {
    kill "$daemon_pid" 2>/dev/null || true
    wait "$daemon_pid" 2>/dev/null || true
    rm -rf "$tmp"
}
trap cleanup EXIT

for _ in $(seq 1 100); do
    if grep -q 'LAN discovery listener bound at 10.42.0.1:8766' "$tmp/server.log"; then
        break
    fi
    if ! kill -0 "$daemon_pid" 2>/dev/null; then
        cat "$tmp/server.log" >&2
        exit 1
    fi
    sleep 0.1
done
grep -q 'LAN discovery listener bound at 10.42.0.1:8766' "$tmp/server.log" || {
    cat "$tmp/server.log" >&2
    exit 1
}

python3 - <<'PY'
import json
import socket
import time

PROBE = b"WAYCONTROL_DISCOVER_V1"
DESTINATION = ("10.42.0.1", 8766)


def client(source="10.42.0.2"):
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.settimeout(0.3)
    if source == "8.8.8.8":
        sock.setsockopt(socket.SOL_IP, socket.IP_FREEBIND, 1)
    sock.bind((source, 0))
    return sock


def expect_silence(sock, message, label):
    sock.sendto(message, DESTINATION)
    try:
        response, _ = sock.recvfrom(512)
    except TimeoutError:
        return
    raise AssertionError(f"{label} unexpectedly received {response!r}")


private = client()
private.sendto(PROBE, DESTINATION)
response, _ = private.recvfrom(512)
announcement = json.loads(response)
assert set(announcement) == {"version", "name", "https_port"}
assert announcement["version"] == 1
assert announcement["https_port"] == 18765
assert len(response) <= 128

# The limiter is keyed by source IP, not UDP source port.
expect_silence(private, PROBE, "immediate repeat")
same_host = client()
expect_silence(same_host, PROBE, "second socket from the same client IP")
same_host.close()

# A separate private client remains eligible while the first client is limited.
other_private = client("10.42.0.3")
other_private.sendto(PROBE, DESTINATION)
other_announcement, _ = other_private.recvfrom(512)
assert json.loads(other_announcement)["https_port"] == 18765

public_source = client("8.8.8.8")
expect_silence(public_source, PROBE, "public source")
expect_silence(private, PROBE + b"!", "malformed probe")

time.sleep(1.05)
private.sendto(PROBE, DESTINATION)
after_window, _ = private.recvfrom(512)
assert json.loads(after_window)["https_port"] == 18765

private.close()
other_private.close()
public_source.close()
print("PASS: isolated LAN discovery response, payload bounds, per-IP rate limit, malformed probe rejection, public-source rejection, and rate-window expiry")
PY
NAMESPACE
