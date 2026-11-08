#!/usr/bin/env python3
"""Exercise the release daemon against this user's live Hyprland session."""
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
PORT = int(os.environ.get("WAYCONTROL_TEST_PORT", "18765"))


def proc_metrics(pid):
    status = pathlib.Path(f"/proc/{pid}/status").read_text()
    rss = next(int(line.split()[1]) for line in status.splitlines() if line.startswith("VmRSS:"))
    stat = pathlib.Path(f"/proc/{pid}/stat").read_text()
    fields = stat[stat.rfind(")") + 2 :].split()
    ticks = int(fields[11]) + int(fields[12])
    return rss, ticks


def main():
    with tempfile.TemporaryDirectory(prefix="h-") as temp:
        base = pathlib.Path(temp)
        config = base / "config"
        # Keep the absolute path short enough for AF_UNIX's sun_path limit even
        # when the real Hyprland instance signature is appended to it.
        runtime = base / "r"
        config.mkdir(mode=0o700)
        runtime.mkdir(mode=0o700)
        original_runtime = os.environ.get("XDG_RUNTIME_DIR")
        if original_runtime and (pathlib.Path(original_runtime) / "hypr").exists():
            (runtime / "hypr").symlink_to(pathlib.Path(original_runtime) / "hypr")
        if original_runtime:
            for socket_name in ("pipewire-0", "pipewire-0-manager"):
                socket_path = pathlib.Path(original_runtime) / socket_name
                if socket_path.exists():
                    (runtime / socket_name).symlink_to(socket_path)
        env = dict(os.environ, XDG_CONFIG_HOME=str(config), XDG_RUNTIME_DIR=str(runtime), WAYCONTROL_BIND_ADDR=f"127.0.0.1:{PORT}")
        proc = subprocess.Popen([str(BINARY), "serve"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)

        def admin(action, ident=None):
            stream = socket.socket(socket.AF_UNIX)
            stream.settimeout(3)
            stream.connect(str(runtime / "waycontrol-admin.sock"))
            stream.sendall(json.dumps({"action": action, "id": ident}).encode() + b"\n")
            stream.shutdown(socket.SHUT_WR)
            response = bytearray()
            while True:
                chunk = stream.recv(4096)
                if not chunk:
                    break
                response.extend(chunk)
            stream.close()
            return json.loads(response)

        try:
            for _ in range(100):
                if proc.poll() is not None:
                    raise RuntimeError(proc.stderr.read().decode())
                try:
                    pairing = admin("pairing_info")
                    break
                except (ConnectionRefusedError, FileNotFoundError, socket.timeout):
                    time.sleep(0.1)
            else:
                raise RuntimeError("daemon admin socket did not become ready")

            cert_path = config / "waycontrol/server-cert.pem"
            tls = ssl.create_default_context(cafile=str(cert_path))

            def request(method, path, payload=None, token=None):
                connection = http.client.HTTPSConnection("127.0.0.1", PORT, context=tls, timeout=5)
                body = None if payload is None else json.dumps(payload)
                headers = {"Content-Type": "application/json"}
                if token:
                    headers["Authorization"] = "Bearer " + token
                connection.request(method, path, body, headers)
                response = connection.getresponse()
                data = response.read()
                status = response.status
                connection.close()
                try:
                    return status, json.loads(data)
                except (ValueError, TypeError):
                    return status, data

            # Idle resource sample over a bounded three-second interval.
            rss_before, ticks_before = proc_metrics(proc.pid)
            time.sleep(3)
            rss_after, ticks_after = proc_metrics(proc.pid)
            idle_cpu = (ticks_after - ticks_before) / os.sysconf("SC_CLK_TCK") / 3 * 100

            status, _ = request("POST", "/api/v1/request", {"version": 1, "id": "unauth", "type": "desktop.info"})
            assert status == 401, f"unauthenticated request returned {status}"
            status, _ = request("POST", "/api/v1/request", {"version": 1, "id": "unauth-close", "type": "window.close", "payload": {"address": "0xdeadbeef"}})
            assert status == 401, f"unauthenticated window control returned {status}"
            status, created = request("POST", "/api/v1/pair", {"code": pairing["pairing"]["code"], "device_name": "Integration test device"})
            assert status == 202, f"pairing request returned {status}"
            pending = admin("pending")["pending"]
            request_id = next(item["request_id"] for item in pending if item["device_name"] == "Integration test device")
            approval = subprocess.run([str(BINARY), "approve", request_id], input="y\n", text=True, capture_output=True, env=env, timeout=5)
            assert approval.returncode == 0, approval.stderr + approval.stdout
            status, paired = request("GET", "/api/v1/pair/" + created["request_id"])
            assert status == 200 and paired["status"] == "approved", f"pair status {status}: {paired}"
            token = paired["token"]

            def api(op, request_id, payload=None):
                body = {"version": 1, "id": request_id, "type": op}
                if payload is not None:
                    body["payload"] = payload
                return request("POST", "/api/v1/request", body, token)

            status, audio = api("audio.info", "audio-state")
            assert status == 200 and audio["success"], f"audio state query failed: {status}"
            assert 0 <= audio["payload"]["output_volume_percent"] <= 100, "audio volume is outside 0-100%"
            assert isinstance(audio["payload"]["output_muted"], bool), "audio mute state is missing"
            assert audio["payload"].get("microphone_muted") is None or isinstance(audio["payload"]["microphone_muted"], bool), "microphone mute state has an invalid value"
            for field in ("output_devices", "input_devices"):
                devices = audio["payload"].get(field)
                assert isinstance(devices, list), f"{field} is missing"
                assert all(isinstance(item.get("id"), int) and isinstance(item.get("name"), str) and isinstance(item.get("active"), bool) for item in devices), f"{field} contains an invalid device"
            status, media = api("media.info", "media-state")
            assert status == 200 and media["success"], f"media player query failed: {status}"
            players = media["payload"].get("players")
            assert isinstance(players, list), "MPRIS player list is missing"
            assert all(
                all(isinstance(player.get(field), str) for field in ("player_id", "name", "playback_status", "title", "artist", "album"))
                for player in players
            ), "MPRIS player metadata has an invalid shape"

            def event_probe(target_workspace, restore_workspace):
                connection = http.client.HTTPSConnection("127.0.0.1", PORT, context=tls, timeout=8)
                connection.request("GET", "/api/v1/events", headers={
                    "Accept": "text/event-stream",
                    "Authorization": "Bearer " + token,
                })
                response = connection.getresponse()
                if response.status != 200:
                    message = response.read().decode(errors="replace")
                    connection.close()
                    raise AssertionError(f"event subscription returned {response.status}: {message}")
                try:
                    status, switched = api("workspace.switch", "event-trigger", {"workspace_id": target_workspace})
                    assert status == 200 and switched["success"], f"event trigger workspace switch returned {status}"
                    deadline = time.monotonic() + 8
                    event = None
                    while time.monotonic() < deadline:
                        line = response.readline()
                        if not line:
                            break
                        if line.startswith(b"data:"):
                            event = line.decode(errors="replace").removeprefix("data:").strip()
                            break
                    assert event and "workspace>>" in event, f"did not receive a Hyprland workspace event: {event!r}"
                    return event
                finally:
                    status, restored = api("workspace.switch", "event-restore", {"workspace_id": restore_workspace})
                    connection.close()
                    assert status == 200 and restored["success"], f"failed to restore active workspace {restore_workspace}"

            status, snapshot = api("desktop.info", "telemetry")
            assert status == 200 and snapshot["success"] and snapshot["payload"].get("computer_name"), "desktop telemetry unavailable"
            telemetry = snapshot["payload"]
            assert isinstance(telemetry.get("load_average"), dict), "load averages unavailable"
            assert set(("one_minute", "five_minutes", "fifteen_minutes")).issubset(telemetry["load_average"]), "load average fields missing"
            assert isinstance(telemetry.get("cpu_cores"), list), "per-core CPU telemetry unavailable"
            assert all(
                item.get("id", "").startswith("cpu") and 0 <= item.get("usage_percent", -1) <= 100
                and (item.get("frequency_khz") is None or item["frequency_khz"] > 0)
                for item in telemetry["cpu_cores"]
            ), "per-core CPU data malformed"
            core_temperatures = telemetry.get("cpu_core_temperatures")
            assert isinstance(core_temperatures, list), "per-core CPU temperature list unavailable"
            assert all(
                item.get("core_id", -1) >= 0
                and (item.get("package_id") is None or item["package_id"] >= 0)
                and -100_000 <= item.get("temperature_millidegrees", -273_151) <= 250_000
                for item in core_temperatures
            ), "per-core CPU temperature data malformed"
            print(f"Per-core CPU temperature sensors: {len(core_temperatures)}")
            status, workspaces = api("workspace.list", "workspaces")
            assert status == 200 and workspaces["success"] and isinstance(workspaces["payload"], list), f"workspace list returned {status}"
            status, windows = api("window.list", "windows")
            assert status == 200 and windows["success"] and isinstance(windows["payload"], list), f"window list returned {status}"
            status, monitors = api("monitor.list", "monitors")
            assert status == 200 and monitors["success"] and isinstance(monitors["payload"], list), f"monitor list returned {status}"
            active = next((item for item in workspaces["payload"] if item["active"]), None)
            live_event = None
            if active:
                status, response = api("workspace.switch", "switch-current", {"workspace_id": active["id"]})
                assert status == 200 and response["success"], f"switch to currently active workspace returned {status}"
                live_event = event_probe(max((item["id"] for item in workspaces["payload"]), default=0) + 5000, active["id"])
            focused = next((item for item in windows["payload"] if item["focused"]), None)
            if focused:
                status, response = api("window.focus", "focus-current", {"address": focused["address"]})
                assert status == 200 and response["success"], f"focus already focused window returned {status}"

            timings = []
            cpu_before, _ = proc_metrics(proc.pid)
            start_cpu = proc_metrics(proc.pid)[1]
            for index in range(20):
                started = time.perf_counter()
                status, response = api("desktop.info", f"perf-{index}")
                timings.append((time.perf_counter() - started) * 1000)
                assert status == 200 and response["success"], f"telemetry sample failed: {status}"
            elapsed = sum(timings) / 1000
            cpu_after = proc_metrics(proc.pid)[1]
            telemetry_cpu = max(0, cpu_after - start_cpu) / os.sysconf("SC_CLK_TCK") / elapsed * 100
            rss_burst, _ = proc_metrics(proc.pid)

            # Fresh connection after a completed response represents a dropped connection/reconnect.
            reconnect_started = time.perf_counter()
            status, pong = api("ping", "reconnect")
            reconnect_latency = (time.perf_counter() - reconnect_started) * 1000
            assert status == 200 and pong["success"], f"reconnect failed: {status}"
            status, _ = api("ping", "invalid id")
            assert status == 400, f"malformed request returned {status}"
            status, _ = api("window.focus", "missing-window", {"address": "0xdeadbeef"})
            assert status == 503, f"missing window should be reported as unavailable, got {status}"
            connection = http.client.HTTPSConnection("127.0.0.1", PORT, context=tls, timeout=5)
            connection.request("POST", "/api/v1/request", b"x" * 70000, {"Authorization": "Bearer " + token, "Content-Type": "application/json"})
            oversized = connection.getresponse().status
            connection.close()
            assert oversized == 413, f"oversized message returned {oversized}"

            device_id = next(device["device_id"] for device in admin("devices")["devices"] if device["name"] == "Integration test device")
            revoked = subprocess.run([str(BINARY), "revoke", device_id], input="y\n", text=True, capture_output=True, env=env, timeout=5)
            assert revoked.returncode == 0, revoked.stderr + revoked.stdout
            status, _ = request("POST", "/api/v1/request", {"version": 1, "id": "revoked", "type": "ping"}, token)
            assert status == 401, f"revoked credential remained accepted: {status}"

            gpu = snapshot["payload"].get("gpu", {})
            print("PASS: pinned-certificate TLS, pairing and local approval, authentication including privileged window control, live telemetry, PipeWire audio state/device lists, MPRIS player discovery/metadata, workspace/window/monitor reads, live Hyprland event stream, same-state workspace/focus controls, reconnect, malformed/oversized requests, missing-window handling, revocation")
            print(f"Host state: {len(workspaces['payload'])} workspaces, {len(windows['payload'])} windows, {len(monitors['payload'])} monitors; GPU available={gpu.get('available')}")
            if live_event:
                print(f"Live Hyprland event: {live_event}")
            print("Telemetry fields:", ", ".join(sorted(snapshot["payload"].keys())))
            microphone_state = audio["payload"].get("microphone_muted")
            microphone_label = "unavailable" if microphone_state is None else ("muted" if microphone_state else "active")
            print(f"PipeWire output: {audio['payload']['output_volume_percent']}% ({'muted' if audio['payload']['output_muted'] else 'active'}); microphone {microphone_label}; devices {len(audio['payload']['output_devices'])} output/{len(audio['payload']['input_devices'])} input")
            print(f"MPRIS players discovered: {len(players)}")
            print(f"Measured daemon RSS: idle {rss_before} KiB, post-idle {rss_after} KiB, after 20 telemetry calls {rss_burst} KiB")
            print(f"Measured CPU: idle {idle_cpu:.2f}% of one core; during 20 telemetry requests {telemetry_cpu:.2f}% of one core")
            print(f"Measured telemetry latency: mean {sum(timings)/len(timings):.1f} ms, min {min(timings):.1f} ms, max {max(timings):.1f} ms")
            print(f"Measured reconnect request latency on a fresh TLS connection: {reconnect_latency:.1f} ms")
            if gpu.get("available"):
                print("NVML sample:", json.dumps({key: gpu.get(key) for key in ("name", "utilization_percent", "temperature_celsius", "vram_used_bytes", "vram_total_bytes", "power_watts")}, sort_keys=True))
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
            server_log = proc.stderr.read().decode(errors="replace")
            if "token" in locals():
                assert token not in server_log, "server log unexpectedly contains a device credential"


if __name__ == "__main__":
    main()
