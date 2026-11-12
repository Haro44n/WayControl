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
pairing_json="$(XDG_CONFIG_HOME="$tmp/config" XDG_RUNTIME_DIR="$tmp/runtime" HOME="$tmp" "$daemon" pairing-info)"
invalid_code="$(PAIRING_JSON="$pairing_json" python3 -c 'import json, os; code=int(json.loads(os.environ["PAIRING_JSON"])["pairing"]["code"]); print(f"{(code + 1) % 1000000:06d}")')"

WAYCONTROL_TEST_CERT="$tmp/config/waycontrol/server-cert.pem" \
WAYCONTROL_TEST_INVALID_CODE="$invalid_code" python3 - <<'PY'
import json
import os
import socket
import ssl
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


def https_status(source, method, path, body=None):
    raw = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    raw.settimeout(3)
    raw.setsockopt(socket.SOL_IP, socket.IP_FREEBIND, 1)
    raw.bind((source, 0))
    raw.connect(("10.42.0.1", 18765))
    context = ssl.create_default_context(cafile=os.environ["WAYCONTROL_TEST_CERT"])
    tls = context.wrap_socket(raw, server_hostname="10.42.0.1")
    payload = b"" if body is None else json.dumps(body).encode()
    headers = [
        f"{method} {path} HTTP/1.1",
        "Host: 10.42.0.1",
        "Connection: close",
    ]
    if body is not None:
        headers.extend(["Content-Type: application/json", f"Content-Length: {len(payload)}"])
    tls.sendall(("\r\n".join(headers) + "\r\n\r\n").encode() + payload)
    response = bytearray()
    while chunk := tls.recv(4096):
        response.extend(chunk)
    tls.close()
    status_line = response.split(b"\r\n", 1)[0].decode()
    return int(status_line.split()[1])


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

# A client with an untrusted source address still must not read authenticated
# state or pair without the live code; TLS verifies the generated IP SAN.
assert https_status(
    "8.8.8.8", "POST", "/api/v1/request",
    {"version": 1, "id": "unauthenticated", "type": "desktop.info"},
) == 401
invalid_code = os.environ["WAYCONTROL_TEST_INVALID_CODE"]
for attempt in range(1, 10):
    status = https_status(
        "8.8.8.8", "POST", "/api/v1/pair",
        {"code": invalid_code, "device_name": "untrusted network probe"},
    )
    expected = 429 if attempt == 9 else 401
    assert status == expected, f"invalid pairing attempt {attempt} returned HTTP {status}, expected {expected}"

private.close()
other_private.close()
public_source.close()
print("PASS: isolated LAN discovery abuse checks and TLS-protected unauthenticated access/pairing rate limits")
PY
NAMESPACE
