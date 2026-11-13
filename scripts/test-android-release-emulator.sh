#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
cd "$script_dir/../android"

daemon="$repo_root/target/release/waycontrold"
test_root="$(mktemp -d)"
runtime_dir="$test_root/runtime"
mkdir -m 700 "$test_root/config" "$runtime_dir"
export XDG_CONFIG_HOME="$test_root/config"
export XDG_RUNTIME_DIR="$runtime_dir"
export WAYCONTROL_BIND_ADDR=127.0.0.1:18765
daemon_pid=""
approval_pid=""

cleanup() {
  local result=$?
  if [ -n "$approval_pid" ]; then
    kill "$approval_pid" 2>/dev/null || true
    wait "$approval_pid" 2>/dev/null || true
  fi
  if [ -n "$daemon_pid" ]; then
    kill -TERM "$daemon_pid" 2>/dev/null || true
    wait "$daemon_pid" 2>/dev/null || true
  fi
  if [ "$result" -ne 0 ] && [ -f "$test_root/daemon.log" ]; then
    cat "$test_root/daemon.log"
    if [ -f "$test_root/approval.log" ]; then cat "$test_root/approval.log"; fi
  fi
  rm -rf "$test_root"
  exit "$result"
}
trap cleanup EXIT

./gradlew assembleDebugAndroidTest

if [ ! -x "$daemon" ]; then
  echo "The release WayControl daemon was not built at $daemon" >&2
  exit 1
fi
env \
  -u HYPRLAND_INSTANCE_SIGNATURE \
  -u SWAYSOCK \
  -u NIRI_SOCKET \
  -u XDG_CURRENT_DESKTOP \
  -u XDG_SESSION_DESKTOP \
  "$daemon" serve >"$test_root/daemon.log" 2>&1 &
daemon_pid=$!

pairing_info=""
for attempt in $(seq 1 100); do
  if ! kill -0 "$daemon_pid" 2>/dev/null; then
    cat "$test_root/daemon.log" >&2
    echo "WayControl daemon exited before its admin socket became ready" >&2
    exit 1
  fi
  if pairing_info="$("$daemon" pairing-info 2>/dev/null)"; then break; fi
  sleep 0.2
done
if [ -z "$pairing_info" ]; then
  cat "$test_root/daemon.log" >&2
  echo "WayControl pairing information did not become available" >&2
  exit 1
fi

readarray -t pairing_values < <(python3 -c 'import json,sys; p=json.load(sys.stdin); print("https://" + p["server_address"]); print(p["certificate_sha256"]); print(p["pairing"]["code"])' <<<"$pairing_info")
if [ "${#pairing_values[@]}" -ne 3 ]; then
  echo "WayControl returned incomplete Android pairing details" >&2
  exit 1
fi
endpoint="${pairing_values[0]}"
fingerprint="${pairing_values[1]}"
pairing_code="${pairing_values[2]}"
computer_name="$(tr -d '\r\n' </etc/hostname)"
if [ -z "$computer_name" ]; then
  echo "The CI host has no hostname to verify in the Android dashboard" >&2
  exit 1
fi
test_port="${WAYCONTROL_BIND_ADDR##*:}"
printf '::add-mask::%s\n' "$pairing_code"
adb reverse "tcp:$test_port" "tcp:$test_port"

(
  for attempt in $(seq 1 450); do
    pending="$("$daemon" pending 2>/dev/null || true)"
    request_id="$(python3 -c 'import json,sys; p=json.load(sys.stdin); print(next((d["request_id"] for d in p.get("pending",[]) if d.get("device_name")=="Android device"), ""))' <<<"$pending" 2>/dev/null || true)"
    if [ -n "$request_id" ]; then
      printf 'y\n' | "$daemon" approve "$request_id"
      exit $?
    fi
    sleep 0.2
  done
  echo "Android pairing request did not arrive before the test deadline" >&2
  exit 1
) >"$test_root/approval.log" 2>&1 &
approval_pid=$!

./gradlew connectedDebugAndroidTest \
  -Pandroid.testInstrumentationRunnerArguments.waycontrolEndpoint="$endpoint" \
  -Pandroid.testInstrumentationRunnerArguments.waycontrolFingerprint="$fingerprint" \
  -Pandroid.testInstrumentationRunnerArguments.waycontrolPairingCode="$pairing_code" \
  -Pandroid.testInstrumentationRunnerArguments.waycontrolComputerName="$computer_name" \
  -Pandroid.testInstrumentationRunnerArguments.class=dev.waycontrol.AndroidRustDaemonE2ETest
wait "$approval_pid"
approval_pid=""

python3 - app/build/outputs/androidTest-results/connected/debug <<'PY'
import pathlib
import sys
import xml.etree.ElementTree as ET

report_dir = pathlib.Path(sys.argv[1])
reports = list(report_dir.glob("TEST-*.xml"))
target_suites = []
for report in reports:
    root = ET.parse(report).getroot()
    target_suites.extend(
        suite for suite in root.findall(".//testsuite")
        if suite.attrib.get("name") == "dev.waycontrol.AndroidRustDaemonE2ETest"
    )
if not target_suites:
    raise SystemExit(f"Android/Rust pairing test report was not produced in {report_dir}")
if len(target_suites) != 1:
    raise SystemExit(f"Expected one Android/Rust pairing test report, found {len(target_suites)}")
suite = target_suites[0]
if int(suite.attrib.get("tests", "0")) < 1 or int(suite.attrib.get("skipped", "0")) or int(suite.attrib.get("failures", "0")) or int(suite.attrib.get("errors", "0")):
    raise SystemExit("Android/Rust pairing E2E test was skipped or failed")
print("Android/Rust pairing E2E test ran and passed")
PY

./gradlew connectedDebugAndroidTest

installed_packages="$(adb shell pm list packages --user 0 | tr -d '\r')"
if grep -Fxq 'package:dev.waycontrol' <<<"$installed_packages"; then
  adb shell am force-stop dev.waycontrol
  adb shell pm uninstall --user 0 dev.waycontrol
fi

adb install app/build/outputs/apk/release/app-release.apk
adb shell monkey -p dev.waycontrol -c android.intent.category.LAUNCHER 1

for attempt in $(seq 1 20); do
  app_pid="$(adb shell pidof dev.waycontrol 2>/dev/null | tr -d '\r' || true)"
  if [ -n "$app_pid" ]; then
    echo "Signed minified release app running with PID $app_pid"
    if [ -n "${WAYCONTROL_SCREENSHOT_PATH:-}" ]; then
      dashboard_screenshot_path="${WAYCONTROL_SCREENSHOT_PATH%.png}-dashboard.png"
      mkdir -p "$(dirname -- "$WAYCONTROL_SCREENSHOT_PATH")"
      capture_screenshot() {
        local screenshot_path="$1"
        adb exec-out screencap -p >"$screenshot_path"
        python3 - "$screenshot_path" <<'PY'
import pathlib
import struct
import sys

image = pathlib.Path(sys.argv[1]).read_bytes()
if len(image) < 8192 or not image.startswith(b"\x89PNG\r\n\x1a\n"):
    raise SystemExit("Android screenshot is missing or is not a valid PNG")
width, height = struct.unpack(">II", image[16:24])
if width < 320 or height < 480:
    raise SystemExit(f"Android screenshot dimensions are unexpectedly small: {width}x{height}")
print(f"Captured WayControl Android screen: {width}x{height}, {len(image)} bytes")
PY
      }

      # Let the first-run route load and its bounded LAN discovery finish so
      # the uploaded onboarding screenshot shows the settled empty state.
      sleep 5
      capture_screenshot "$WAYCONTROL_SCREENSHOT_PATH"

      # The pixel_2 CI profile is 1080x1920; this is the center of the
      # first-run "Continue without pairing" button on that profile.
      adb shell input tap 316 557
      sleep 2
      capture_screenshot "$dashboard_screenshot_path"
    fi
    exit 0
  fi
  sleep 1
done

adb logcat -d -s AndroidRuntime:E
echo "Signed minified release app did not remain running after launch" >&2
exit 1
