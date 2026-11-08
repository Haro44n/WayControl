# WayControl

**Your desktop. Your control.**

WayControl is a native Android companion for Wayland desktops. The Hyprland adapter provides authenticated workspace, window, monitor, and event control, with a live loopback integration run. The Sway adapter implements workspace, window, output, and event IPC; the Niri adapter implements workspace, window, output, and event IPC. Both have protocol-fixture tests and still need live-session validation. Niri fullscreen toggles and per-monitor power remain unavailable because its current IPC model does not expose fullscreen state or per-output power control. KDE KWin supports workspace listing, switching, and workspace-change events through its D-Bus API; window and monitor controls remain unavailable. GNOME sessions are detected but compositor controls remain unsupported; no general remote-control interface was identified, and GNOME integration needs further investigation. The authenticated HTTPS daemon, explicit Linux-side pairing approval, typed allowlist API, and live Linux telemetry do not depend on a compositor. WayControl does not execute arbitrary remote shell commands.

## Release status: PARTIAL

The Rust daemon and Android debug/release variants build. A live loopback integration run verified pinned-certificate TLS pairing, local approval, real Hyprland state and events, per-core CPU utilization/frequency and load averages, PipeWire audio state/device lists, MPRIS player discovery/metadata, monitor reads, NVML data, reconnect, request rejection, and revocation. Android 16 emulator testing verified secure pairing, live data, navigation, credential restore, LAN-discovery UI, QR entry controls, and daemon reconnection at a Poco M5-sized resolution; physical phone verification remains. The System screen lists discovered media players and their metadata, with player-targeted play/pause, next, and previous controls. Power, monitor, volume, output/microphone mute, and PipeWire device selection are also implemented. Settings lets the user choose and persist 1, 2, or 5-second Home/System telemetry refresh intervals; other tabs poll five times less often, and polling pauses when the app leaves the foreground. System Monitor shows Linux load averages and per-core CPU usage/frequency when available. Automation profiles remain. See [PROGRESS.md](PROGRESS.md) for test evidence, measured performance, APKs, and remaining work.

## Build

Rust (workspace root):

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
dbus-run-session -- cargo test --workspace
cargo build --workspace --release
```

The Rust test suite starts an isolated session bus for the KWin D-Bus adapter fixtures; install `dbus-daemon` to get `dbus-run-session`.

Android (JDK 17 or newer, Android SDK platform 37 and Build Tools installed):

```sh
cd android
./gradlew testDebugUnitTest lintDebug assembleDebug
./gradlew assembleRelease
# With an Android emulator running, execute the Compose UI tests too:
./gradlew connectedDebugAndroidTest
```

The Android suite includes TLS pin and request-envelope unit tests, telemetry interval scheduling, snapshot clearing after disconnect, conservative compositor capability parsing/fetch-plan/navigation tests, and Compose tests for refresh selection, load-average/per-core display, and dangerous-action confirmation.

The debug APK is written to `android/app/build/outputs/apk/debug/app-debug.apk`. A copy is available at `outputs/WayControl-v0.1.0-debug.apk`; the minified unsigned release build is at `outputs/WayControl-v0.1.0-release-unsigned.apk`. Release signing is intentionally not configured with a checked-in key; configure signing through a private local Gradle configuration before publishing.

## Run the Linux companion

On Arch Linux, build and install the local x86_64 package with `./packaging/arch/build-package.sh --syncdeps --install`, then enable the user service with `systemctl --user enable --now waycontrol.service`. A prebuilt package is available at [outputs/WayControl-v0.1.0-1-x86_64.pkg.tar.zst](outputs/WayControl-v0.1.0-1-x86_64.pkg.tar.zst). The package build and manual setup are documented in [Arch installation and pairing](docs/INSTALL_ARCH.md).

Build and install `target/release/waycontrold` to `~/.local/bin/waycontrold`, then install the user service:

```sh
mkdir -p ~/.config/waycontrol ~/.config/systemd/user ~/.local/bin
install -m755 target/release/waycontrold ~/.local/bin/waycontrold
install -m644 systemd/waycontrol.service ~/.config/systemd/user/waycontrol.service
systemctl --user daemon-reload
systemctl --user enable --now waycontrol.service
```

The service binds to loopback by default. For a phone on the LAN, edit the service's `WAYCONTROL_BIND_ADDR` to the desktop's private LAN address, for example `192.168.1.20:8765`, then restart the service. The daemon certificate includes that address as a name when first generated. When bound to a specific private IPv4 address, the daemon also listens for local discovery probes on UDP port 8766. Android's “Find on local network” fills only the HTTPS address; it does not trust the announced machine or change the certificate fingerprint. Enter the SHA-256 fingerprint shown by `waycontrold pairing-info` and the short-lived code, then approve the pairing in the Linux terminal. Discovery is disabled for loopback, wildcard, public, and IPv6 binds. Limit HTTPS and UDP exposure with the host firewall; WayControl does not change firewall configuration. Existing `~/.config/hyprcontrol` daemon state is migrated to `~/.config/waycontrol` on first startup after upgrading.

Run `waycontrold pairing-info --qr` in the Linux terminal to display a QR code containing the private-LAN HTTPS address, certificate SHA-256 fingerprint, and short-lived pairing code. In Android Settings, scan the QR with Google Play services or enter the same values manually. The QR contains no trusted-device token and expires with the pairing code; Linux approval remains required. QR mode requires the daemon to bind to a specific private or link-local IPv4 address. Run `waycontrold pairing-info` for the JSON details without a QR. Run `waycontrold pending` and then `waycontrold approve REQUEST_ID` to approve the displayed phone by name. Revoke with `waycontrold devices` and `waycontrold revoke DEVICE_ID`. Approval and revocation require an interactive `y` confirmation.

See [Arch installation and pairing](docs/INSTALL_ARCH.md) and [Security status](SECURITY.md).

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
