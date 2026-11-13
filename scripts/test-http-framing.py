#!/usr/bin/env python3
"""Test authenticated HTTP/1.1 framing against an isolated daemon."""
import http.client
import json
import os
import pathlib
import socket
import ssl
import subprocess
import tempfile
import time


ROOT = pathlib.Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/release/waycontrold"
MAX_CONCURRENT_API_CONNECTIONS = 64


def open_idle_tls(context, port):
    raw = socket.create_connection(("127.0.0.1", port), timeout=5)
    return context.wrap_socket(raw, server_hostname="127.0.0.1")


def main():
    with tempfile.TemporaryDirectory(prefix="wc-http-") as temp:
        root = pathlib.Path(temp)
        config = root / "config"
        runtime = root / "runtime"
        config.mkdir(mode=0o700)
        runtime.mkdir(mode=0o700)
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            port = probe.getsockname()[1]

        env = dict(os.environ)
        env.update(
            XDG_CONFIG_HOME=str(config),
            XDG_RUNTIME_DIR=str(runtime),
            WAYCONTROL_BIND_ADDR=f"127.0.0.1:{port}",
        )
        for key in (
            "HYPRLAND_INSTANCE_SIGNATURE",
            "SWAYSOCK",
            "NIRI_SOCKET",
            "WAYLAND_DISPLAY",
            "XDG_CURRENT_DESKTOP",
            "XDG_SESSION_DESKTOP",
        ):
            env.pop(key, None)

        proc = subprocess.Popen(
            [str(BINARY), "serve"],
            env=env,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
        )

        def admin(action, ident=None):
            connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            connection.settimeout(2)
            connection.connect(str(runtime / "waycontrol-admin.sock"))
            connection.sendall(json.dumps({"action": action, "id": ident}).encode() + b"\n")
            connection.shutdown(socket.SHUT_WR)
            response = bytearray()
            while len(response) <= 8192:
                chunk = connection.recv(4096)
                if not chunk:
                    break
                response.extend(chunk)
            connection.close()
            if len(response) > 8192:
                raise AssertionError("local administration response exceeded its limit")
            return json.loads(response)

        token = None
        try:
            for _ in range(100):
                if proc.poll() is not None:
                    raise RuntimeError(proc.stderr.read().decode(errors="replace"))
                try:
                    pairing = admin("pairing_info")
                    break
                except (ConnectionRefusedError, FileNotFoundError, socket.timeout):
                    time.sleep(0.05)
            else:
                raise RuntimeError("daemon administration socket did not become ready")

            certificate = config / "waycontrol/server-cert.pem"
            tls = ssl.create_default_context(cafile=str(certificate))
            request = http.client.HTTPSConnection("127.0.0.1", port, context=tls, timeout=5)
            request.request(
                "POST",
                "/api/v1/pair",
                json.dumps({"code": pairing["pairing"]["code"], "device_name": "HTTP framing test"}),
                {"Content-Type": "application/json"},
            )
            response = request.getresponse()
            assert response.status == 202, f"pairing returned HTTP {response.status}"
            created = json.loads(response.read())
            request.close()

            approval = subprocess.run(
                [str(BINARY), "approve", created["request_id"]],
                input="y\n",
                text=True,
                capture_output=True,
                env=env,
                timeout=5,
            )
            assert approval.returncode == 0, approval.stderr + approval.stdout
            approved = http.client.HTTPSConnection("127.0.0.1", port, context=tls, timeout=5)
            approved.request("GET", f"/api/v1/pair/{created['request_id']}")
            response = approved.getresponse()
            pair_status = json.loads(response.read())
            approved.close()
            assert response.status == 200 and pair_status["status"] == "approved", pair_status
            token = pair_status["token"]

            # A peer must not hold one of the bounded TLS slots forever by
            # declaring a large JSON body and then sending only its prefix.
            slow_body = open_idle_tls(tls, port)
            slow_body.settimeout(12)
            slow_body.sendall(
                b"POST /api/v1/pair HTTP/1.1\r\n"
                b"Host: 127.0.0.1\r\n"
                b"Content-Type: application/json\r\n"
                b"Content-Length: 60000\r\n"
                b"Connection: keep-alive\r\n\r\n"
                b'{"code":"'
            )
            try:
                slow_response = bytearray(slow_body.recv(4096))
            except TimeoutError as error:
                raise AssertionError("server left an incomplete JSON request open") from error
            assert slow_response.startswith(b"HTTP/1.1 408"), (
                "incomplete JSON request did not return HTTP 408: "
                + bytes(slow_response[:256]).decode(errors="replace")
            )
            assert b"connection: close" in slow_response.lower(), (
                "timed-out request did not ask the peer to close the connection"
            )
            slow_body.settimeout(2)
            try:
                assert slow_body.recv(1) == b"", "timed-out request connection remained open"
            except (ConnectionResetError, ssl.SSLError):
                pass
            slow_body.close()

            # Completed TLS sessions that never send HTTP must still consume a
            # bounded slot so unauthenticated clients cannot exhaust daemon FDs.
            idle_connections = []
            try:
                for _ in range(MAX_CONCURRENT_API_CONNECTIONS):
                    idle_connections.append(open_idle_tls(tls, port))
                try:
                    excess = open_idle_tls(tls, port)
                except (OSError, TimeoutError):
                    pass
                else:
                    excess.close()
                    raise AssertionError("server accepted more than 64 concurrent TLS connections")

                idle_connections.pop().close()
                deadline = time.monotonic() + 2
                while True:
                    try:
                        recovered = open_idle_tls(tls, port)
                        recovered.close()
                        break
                    except (OSError, TimeoutError):
                        if time.monotonic() >= deadline:
                            raise AssertionError("closing a TLS connection did not release its slot")
                        time.sleep(0.01)
            finally:
                for connection in idle_connections:
                    connection.close()

            first_body = json.dumps(
                {"version": 1, "id": "framing-first", "type": "ping"}, separators=(",", ":")
            ).encode()
            second_body = json.dumps(
                {"version": 1, "id": "framing-second", "type": "ping"}, separators=(",", ":")
            ).encode()
            chunked_body = f"{len(first_body):x}\r\n".encode() + first_body + b"\r\n0\r\n\r\n"
            pipelined_request = (
                b"POST /api/v1/request HTTP/1.1\r\n"
                b"Host: 127.0.0.1\r\n"
                b"Content-Type: application/json\r\n"
                + f"Authorization: Bearer {token}\r\n".encode()
                + f"Content-Length: {len(second_body)}\r\n\r\n".encode()
                + second_body
            )
            hostile_request = (
                b"POST /api/v1/request HTTP/1.1\r\n"
                b"Host: 127.0.0.1\r\n"
                b"Content-Type: application/json\r\n"
                + f"Authorization: Bearer {token}\r\n".encode()
                + b"Content-Length: 0\r\nTransfer-Encoding: chunked\r\n\r\n"
                + chunked_body
                + pipelined_request
            )
            with socket.create_connection(("127.0.0.1", port), timeout=5) as raw:
                with tls.wrap_socket(raw, server_hostname="127.0.0.1") as connection:
                    connection.settimeout(5)
                    connection.sendall(hostile_request)
                    hostile_response = bytearray()
                    peer_closed = False
                    try:
                        while len(hostile_response) <= 8192:
                            chunk = connection.recv(4096)
                            if not chunk:
                                peer_closed = True
                                break
                            hostile_response.extend(chunk)
                    except (ConnectionResetError, ssl.SSLError):
                        peer_closed = True
            assert len(hostile_response) <= 8192, "conflicting framing produced an oversized response"
            assert peer_closed, "server kept the connection open after conflicting HTTP framing"
            assert hostile_response.count(b"HTTP/1.1 ") == 1, (
                "server processed a pipelined request after conflicting HTTP framing: "
                + bytes(hostile_response[:512]).decode(errors="replace")
            )

            fresh = http.client.HTTPSConnection("127.0.0.1", port, context=tls, timeout=5)
            fresh.request(
                "POST",
                "/api/v1/request",
                json.dumps({"version": 1, "id": "framing-recovery", "type": "ping"}),
                {"Content-Type": "application/json", "Authorization": f"Bearer {token}"},
            )
            response = fresh.getresponse()
            result = json.loads(response.read())
            fresh.close()
            assert response.status == 200 and result["success"], "fresh connection failed after framing probe"
            print(
                "PASS: pairing, incomplete-body timeout, 64-connection cap and recovery, "
                "authenticated ping, and HTTP framing rejection"
            )
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
            server_log = proc.stderr.read().decode(errors="replace")
            if token is not None:
                assert token not in server_log, "server log unexpectedly contains the paired credential"


if __name__ == "__main__":
    main()
