#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$script_dir/../android"

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
      mkdir -p "$(dirname -- "$WAYCONTROL_SCREENSHOT_PATH")"
      sleep 2
      adb exec-out screencap -p >"$WAYCONTROL_SCREENSHOT_PATH"
      python3 - "$WAYCONTROL_SCREENSHOT_PATH" <<'PY'
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
    fi
    exit 0
  fi
  sleep 1
done

adb logcat -d -s AndroidRuntime:E
echo "Signed minified release app did not remain running after launch" >&2
exit 1
