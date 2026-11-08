# Security status

## Implemented

- The daemon uses HTTPS with a persistent self-signed server certificate. Android requires the exact SHA-256 leaf-certificate fingerprint supplied through the local CLI and retains the platform hostname check.
- All desktop API operations require a randomly generated bearer credential. Pairing requires a short-lived code and explicit interactive approval from the Linux user's local CLI.
- Linux stores only a SHA-256 digest of each device credential. The device database and TLS private key live under the user's configuration directory with restrictive permissions. Android encrypts the credential with AES-GCM and a non-exportable Android Keystore key.
- Pairing attempts are rate-limited, pending requests and codes expire, API bodies are capped at 64 KiB, and revocation invalidates a device credential.
- Remote actions are represented by typed protocol operations and validated identifiers. No arbitrary command or shell operation is exposed. System actions use fixed executable names and argument allowlists; closing windows, powering off monitors, and system power actions require Android confirmation.
- The authenticated Hyprland event stream is bounded to four concurrent clients and limits event lines to 8 KiB.
- Local approval, listing, and revocation use a mode-0600 Unix socket in the user's runtime directory. The server binds to loopback by default.

## Remaining security work

- Android pairing, pinned TLS, Keystore-backed token restore, and daemon reconnect were exercised on an Android 16 emulator. Physical Poco M5 and additional API/OEM combinations remain untested. Manual address and certificate entry is used; there is no QR flow yet.
- LAN binding and firewall exposure require deliberate operator configuration. No port discovery, LAN service advertisement, or firewall changes are performed by the installer.
- A formal security review, fuzzing of the network parser, and validation on multiple Android API levels are still needed.
- Power and monitor operations are available only to paired devices, but their execution has not been tested on the live desktop. Lock, systemctl power commands, wpctl, playerctl, and DPMS availability depend on the user's session installation. Server-side policy currently validates authentication and allowlisted values; confirmation is a client UI behavior.
- The daemon can be configured to bind to a LAN address. It does not discover or firewall-protect that address; operators must restrict access to trusted networks.

## Review performed (2026-11-07)

This was a source-level review of the Rust HTTP/admin paths, Android TLS and credential storage, systemd unit, plus the Rust and live loopback security tests. It is not an independent penetration test or a complete security certification.

The review found and fixed these issues:

- **Revoked event-stream credentials remained usable while idle (high impact, fixed):** the stream now rechecks the device credential for each event and on a five-second heartbeat. A regression test revokes a device while its stream is idle and verifies closure.
- **Authenticated API clients had no request rate limit (medium impact, fixed):** authenticated operations are limited to 30 requests per device per second and return HTTP 429 after the limit. A route-level regression test covers the rejection.
- **Approved credentials could be cached by an intermediary (medium impact, fixed):** pairing-status responses now include `Cache-Control: no-store`; a test verifies the header on the response containing the token.
- **Pairing display names could use terminal and Unicode direction controls (low impact, fixed):** accepted names are restricted to printable ASCII and spaces, with a length cap. Tests cover ANSI escapes and a bidi override.
- **A user service could start without the Hyprland instance environment (reliability issue, fixed):** the backend now discovers a unique active socket pair and retries discovery after the compositor appears. Ambiguous sessions fail closed. Multiple-session behavior remains to be exercised on a host configured with multiple Hyprland sessions.

Security checks exercised in the current Rust test and loopback runs include expired pairing-code rejection, explicit local approval, unauthenticated control denial, revoked API credential rejection, idle-stream closure after revocation, malformed and oversized request rejection, and checking daemon output for the paired bearer token. No test invokes actual shutdown, reboot, suspend, or other destructive host controls.

## Audit limits and deployment notes

- `cargo-audit` and `gitleaks` were not installed, so dependency advisory scanning and dedicated secret scanning were not run. No dependency tooling was installed as part of this review.
- Fuzzing and a hostile-network penetration test remain outstanding. The TLS pairing/reconnect path was exercised on an Android emulator, not across a hostile network or multiple physical Android vendors.
- `systemd-analyze verify systemd/waycontrol.service` parsed the unit but reported its configured `%h/.local/bin/waycontrold` executable is absent because the service has not been installed. The service itself was not started under systemd.
- LAN binding is opt-in and must be paired with host firewall rules for a trusted network. The project does not configure firewall policy.
- `PROGRESS.md` records the complete test evidence, measured runtime data, and product work that remains before v0.1 acceptance.

Report suspected vulnerabilities through the project maintainer. Do not expose a LAN-bound service on an untrusted network.
