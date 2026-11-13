# Arch Linux installation and pairing

## Linux daemon

### Install from an Arch package

From the checked-out WayControl source tree on x86_64, install the build tools and build/install the local package:

```sh
sudo pacman -S --needed base-devel cargo
./packaging/arch/build-package.sh --syncdeps --install
mkdir -p ~/.config/waycontrol
systemctl --user daemon-reload
systemctl --user enable --now waycontrol.service
```

The helper fetches the locked Rust dependencies before `makepkg`; the PKGBUILD builds offline and installs `/usr/bin/waycontrold` plus a systemd user unit. The package depends on GTK 3 for the profile application-launch operation (`gtk-launch`). The package source is the current checkout, so rerun the helper after updating it. To build without installing, run `./packaging/arch/build-package.sh` and install the resulting `packaging/arch/waycontrol-*.pkg.tar.zst` later with `sudo pacman -U`.

### Build and install manually

Install Rust with Arch packages or rustup. From the repository root:

```sh
cargo build --locked --release -p waycontrol-server
mkdir -p ~/.local/bin ~/.config/waycontrol ~/.config/systemd/user
install -Dm755 target/release/waycontrold ~/.local/bin/waycontrold
install -Dm644 systemd/waycontrol.service ~/.config/systemd/user/waycontrol.service
systemctl --user daemon-reload
systemctl --user enable --now waycontrol.service
```

The user service runs without root, starts with the graphical session, and listens on loopback at `127.0.0.1:8765`. It uses `HYPRLAND_INSTANCE_SIGNATURE` when available. If that variable is not imported into the systemd user manager, it selects a compositor only when exactly one active Hyprland IPC socket pair exists; with multiple active instances it fails closed and asks for an explicit signature. To target a particular instance, import its signature into the user manager and restart the service:

```sh
systemctl --user import-environment HYPRLAND_INSTANCE_SIGNATURE SWAYSOCK NIRI_SOCKET XDG_CURRENT_DESKTOP XDG_SESSION_DESKTOP XDG_SESSION_TYPE
systemctl --user restart waycontrol.service
```

For LAN pairing, edit `~/.config/systemd/user/waycontrol.service` and change `WAYCONTROL_BIND_ADDR` to the desktop's private IP and port (for example `192.168.1.20:8765`), then run:

```sh
systemctl --user daemon-reload
systemctl --user restart waycontrol.service
```

The daemon detects Hyprland, Sway, Niri, KDE KWin, and GNOME from session variables and compositor socket variables. Hyprland workspace/window/monitor/event controls have live loopback coverage. GitHub Actions also exercises Sway in a headless compositor, Niri in a nested compositor, and KWin with its virtual compositor. These isolated checks cover workspace/window/output operations and event delivery, but do not replace validation in a user's Sway, Niri, or Plasma session. Sway supports moving a window to an exact output from the current Sway output list and monitor power; lock-screen control is unavailable. Niri fullscreen-state, per-monitor power, and lock-screen capabilities are unavailable. KWin window/monitor operations remain unsupported. GNOME Wayland sessions can use the optional GNOME Shell bridge for workspaces and windows; without the extension, or on GNOME X11, compositor capabilities stay unavailable. GNOME floating-state toggles, monitor power, and lock-screen actions are unsupported. GNOME's XDG RemoteDesktop portal can mediate user-approved input, but WayControl does not use it and it does not provide workspace/window management. System telemetry, pairing, networking, audio/media, and power endpoints are independent of workspace/window support.

When a Sway session does not import `SWAYSOCK` into the systemd user manager, WayControl searches the user's `XDG_RUNTIME_DIR` for Sway IPC sockets and probes candidates. It enables Sway controls only when exactly one live socket responds; stale or ambiguous candidates leave compositor controls unavailable.

Hyprland also reports pinned window state and supports pin/unpin for floating windows; tiled windows are rejected by the adapter. The Windows screen lists each window's monitor, offers a move-to-monitor action for available outputs, and exposes centering for floating windows.

### Optional GNOME Shell bridge

The package places extension files in `/usr/share/waycontrol/gnome-shell-extension`; the package does not install them into GNOME's enabled-extension list. The extension supports GNOME Shell 45 through 51. GJS ES modules are used from GNOME Shell 45 onward. Compatibility claims cover the declared API range; CI runs the nested Shell check on the distribution's available GNOME version, which does not replace testing on a user's session.

Install the extension for your user and enable it manually:

```sh
extension_id=waycontrol@haro44n.github.io
install -d "$HOME/.local/share/gnome-shell/extensions/$extension_id"
install -m644 /usr/share/waycontrol/gnome-shell-extension/metadata.json "$HOME/.local/share/gnome-shell/extensions/$extension_id/metadata.json"
install -m644 /usr/share/waycontrol/gnome-shell-extension/extension.js "$HOME/.local/share/gnome-shell/extensions/$extension_id/extension.js"
gnome-extensions enable "$extension_id"
```

To disable the bridge, run `gnome-extensions disable waycontrol@haro44n.github.io`. The extension runs inside GNOME Shell with the user's session authority; only install and enable it if you want this local compositor integration. It exposes a fixed session-bus interface to the per-user WayControl daemon. The interface version is negotiated when the daemon starts, so restart `waycontrol.service` after enabling, disabling, or updating the extension. Changes to this GNOME integration do not enable or reconfigure KDE Connect, portals, Hyprland, Quickshell, or the system firewall.

The interface is `org.waycontrol.GnomeShell1` at `/org/waycontrol/GnomeShell` on `org.waycontrol.GnomeShell`. Methods use fixed signatures: `GetVersion() → u`, `ListWorkspaces() → s`, `ListWindows() → s`, `SwitchWorkspace(s) → b`, `FocusWindow(s) → b`, `MoveWindow(s,s) → b`, `ToggleFullscreen(s) → b`, and `CloseWindow(s) → b`; `Changed(s)` emits only `workspace-changed` or `windows-changed`. Lists are JSON strings capped by the adapter at 256 KiB, 256 workspaces, and 128 windows; individual text values are bounded. IDs are opaque `gnome:<decimal>` values resolved against live Shell objects before each action. The window `floating` field is `null` because Mutter does not provide that state through this bridge. Window titles are desktop data and are returned only through the existing authenticated API.

The address must be an IP address, not `0.0.0.0`, so the generated certificate can include it. Restrict access to the private network with your firewall. Installation does not edit firewall, Hyprland, Quickshell, KDE Connect, portal, or other desktop settings.

## Android debug app

Install `android/app/build/outputs/apk/debug/app-debug.apk` on the phone, or build it with Android SDK platform 37 and JDK 17+:

```sh
cd android
./gradlew testDebugUnitTest lintDebug assembleDebug
```

## Pairing

1. Configure the daemon to bind to its private LAN IPv4 address, then run `waycontrold pairing-info --qr` in the active session. The terminal prints a QR containing the HTTPS address, certificate SHA-256, and short-lived code. Use `waycontrold pairing-info` for readable JSON/manual entry.
2. In WayControl Settings, scan the QR or enter the HTTPS address, fingerprint, and code. The QR carries no trusted-device credential; Android still checks the exact certificate fingerprint and keeps normal hostname verification enabled.
3. On Linux, run `waycontrold pending`, inspect the device name, then run `waycontrold approve REQUEST_ID` and type `y` to approve.
4. The Android app receives its random device credential after approval, encrypts it with an AES-GCM key held by Android Keystore, and reconnects with it on later launches.
5. To remove access, run `waycontrold devices` followed by `waycontrold revoke DEVICE_ID` and confirm with `y`.

Pairing codes expire after three minutes; unapproved requests expire after five minutes. Rejected or expired codes require starting again with the current `pairing-info` code.

QR scanning uses the Google Play services Code Scanner and does not request camera permission. Devices without Google Play services can use manual entry. QR mode supports private or link-local IPv4 addresses; it is unavailable when the daemon binds to loopback, a wildcard, a public IP, or IPv6.

## Troubleshooting

- `Hyprland is unavailable`: run the service as the same user and session as Hyprland; inspect `systemctl --user status waycontrol.service` and `journalctl --user -u waycontrol.service`.
- Certificate mismatch: copy the complete SHA-256 fingerprint shown by the running daemon. Regenerating the certificate changes the fingerprint.
- Connection timeout: confirm the service bind address, phone and desktop are on a reachable network, and the firewall allows the chosen TCP port.
- Android does not connect to the daemon's default loopback address from a phone. Configure the desktop's private LAN IP as described above.
- NVML metrics may be unavailable if NVIDIA drivers do not expose the library or the GPU does not support a specific measurement. Missing values are shown as unavailable.

## Build an unsigned release variant

```sh
cd android
./gradlew assembleRelease
```

The release variant enables R8 shrinking but is not signed for publishing. Use a private signing configuration or CI secret; do not commit signing keys or passwords.
