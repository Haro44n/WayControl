#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$script_dir/../android"

./gradlew connectedDebugAndroidTest

if adb shell pm list packages --user 0 | tr -d '\r' | grep -Fxq 'package:dev.waycontrol'; then
  adb shell am force-stop dev.waycontrol
  adb shell pm uninstall --user 0 dev.waycontrol
fi

adb install app/build/outputs/apk/release/app-release.apk
adb shell monkey -p dev.waycontrol -c android.intent.category.LAUNCHER 1

for attempt in $(seq 1 20); do
  app_pid="$(adb shell pidof dev.waycontrol | tr -d '\r')"
  if [ -n "$app_pid" ]; then
    echo "Signed minified release app running with PID $app_pid"
    exit 0
  fi
  sleep 1
done

adb logcat -d -s AndroidRuntime:E
echo "Signed minified release app did not remain running after launch" >&2
exit 1
