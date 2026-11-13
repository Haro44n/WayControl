#!/usr/bin/env python3
"""Verify the daemon discovers a live Sway IPC socket without imported env vars."""
import json
import os
import pathlib
import socket
import struct
import subprocess
import tempfile
import threading


ROOT = pathlib.Path(__file__).resolve().parents[1]
BINARY = pathlib.Path(
    os.environ.get("WAYCONTROL_DAEMON", ROOT / "target/release/waycontrold")
).resolve()
MAGIC = b"i3-ipc"
GET_WORKSPACES = 1
GET_TREE = 4


def serve_sway_ipc(listener, stop):
    while not stop.is_set():
        try:
            listener.settimeout(0.1)
            stream, _ = listener.accept()
        except TimeoutError:
            continue
        except OSError:
            if stop.is_set():
                return
            raise
        with stream:
            header = bytearray()
            while len(header) < 14:
                chunk = stream.recv(14 - len(header))
                if not chunk:
                    break
                header.extend(chunk)
            if len(header) != 14 or header[:6] != MAGIC:
                continue
            length, message_type = struct.unpack("=II", header[6:14])
            payload = bytearray()
            while len(payload) < length:
                chunk = stream.recv(length - len(payload))
                if not chunk:
                    break
                payload.extend(chunk)
            if len(payload) != length:
                continue
            if message_type == GET_WORKSPACES:
                body = b"[]"
            elif message_type == GET_TREE:
                body = b'{"type":"root","nodes":[],"floating_nodes":[]}'
            else:
                continue
            stream.sendall(MAGIC + struct.pack("=II", len(body), message_type) + body)


def inspect(binary, env):
    result = subprocess.run(
        [str(binary), "inspect"],
        env=env,
        capture_output=True,
        text=True,
        timeout=10,
        check=False,
    )
    if result.returncode != 0:
        raise RuntimeError(f"daemon inspect failed: {result.stderr}\n{result.stdout}")
    return json.loads(result.stdout)


def main():
    if not BINARY.is_file() or not os.access(BINARY, os.X_OK):
        raise RuntimeError(f"daemon executable not found or not executable: {BINARY}")
    with tempfile.TemporaryDirectory(prefix="wc-sway-") as temp:
        runtime = pathlib.Path(temp)
        uid = os.getuid()
        stale_path = runtime / f"sway-ipc.{uid}.{os.getpid() + 100000}.sock"
        stale_listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        stale_listener.bind(str(stale_path))
        stale_listener.close()
        socket_path = runtime / f"sway-ipc.{uid}.{os.getpid()}.sock"
        listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        listener.bind(str(socket_path))
        listener.listen()
        stop = threading.Event()
        server = threading.Thread(target=serve_sway_ipc, args=(listener, stop), daemon=True)
        server.start()

        env = dict(os.environ)
        for name in (
            "SWAYSOCK",
            "HYPRLAND_INSTANCE_SIGNATURE",
            "NIRI_SOCKET",
            "XDG_CURRENT_DESKTOP",
            "XDG_SESSION_DESKTOP",
        ):
            env.pop(name, None)
        env["XDG_RUNTIME_DIR"] = str(runtime)
        report = inspect(BINARY, env)

        assert report["compositor"]["id"] == "sway", report["compositor"]
        assert report["capabilities"]["workspaces"] is True, report["capabilities"]
        assert report["state"]["available"] is True, report["state"]
        assert report["state"]["workspaces"] == [], report["state"]

        second_path = runtime / f"sway-ipc.{uid}.{os.getpid() + 1}.sock"
        second_listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        second_listener.bind(str(second_path))
        second_listener.listen()
        second_stop = threading.Event()
        second_server = threading.Thread(
            target=serve_sway_ipc, args=(second_listener, second_stop), daemon=True
        )
        second_server.start()
        second_report = inspect(BINARY, env)
        stop.set()
        listener.close()
        server.join(timeout=1)
        second_stop.set()
        second_listener.close()
        second_server.join(timeout=1)
        assert second_report["compositor"]["id"] == "unknown", second_report["compositor"]
        assert second_report["capabilities"]["workspaces"] is False, second_report["capabilities"]
        print("PASS: unique Sway socket discovery works without SWAYSOCK and rejects ambiguity")


if __name__ == "__main__":
    main()
