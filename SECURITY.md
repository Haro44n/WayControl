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

- Android pairing, pinned TLS, Keystore-backed token restore, and daemon reconnect were exercised on an Android 16 emulator. The terminal QR generator and Android Google Play services scanner are implemented; Android validates HTTPS, a private IPv4 endpoint, the certificate fingerprint, the short-lived pairing code, and its expiry before filling the pairing form. End-to-end camera scanning on a Google Play-enabled physical device, the Poco M5, and additional API/OEM combinations remain untested.
- LAN binding and firewall exposure require deliberate operator configuration. No port discovery, LAN service advertisement, or firewall changes are performed by the installer.
- Fuzzing currently covers bounded JSON request-envelope parsing and typed validation. Coverage-guided fuzzing does not replace a hostile-network penetration test or live device checks.
- Power and monitor operations are available only to paired devices, but their execution has not been tested on the live desktop. Lock, systemctl power commands, wpctl, playerctl, and DPMS availability depend on the user's session installation. Server-side policy currently validates authentication and allowlisted values; confirmation is a client UI behavior.
- The daemon can be configured to bind to a LAN address. It does not discover or firewall-protect that address; operators must restrict access to trusted networks.
- Allowlisted host commands have a five-second deadline and cap captured output at 64 KiB stdout and 8 KiB stderr. This prevents unbounded command output from exhausting daemon memory; timeout or output-limit cancellation kills the child process.

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

- `cargo audit` scanned 207 locked Rust crate dependencies. It found no vulnerability advisories; it reported three informational unmaintained-crate advisories for `zvariant`, `zvariant_derive`, and `zbus_names` from the current zbus 5 dependency graph. These crates have been folded into zbus 6, but zbus 6 requires Rust 1.89 while this project currently declares Rust 1.85, so the major migration is deferred and the warnings remain recorded.
- `gitleaks git . --redact=100` scanned 19 commits and found no leaks.
- A 45-second libFuzzer run exercised the 64 KiB-bounded JSON `RequestEnvelope` parser and typed validator for 4.1 million executions without a crash. This is not a hostile-network penetration test. The TLS pairing/reconnect path was exercised on an Android emulator, not across a hostile network or multiple physical Android vendors.
- `systemd-analyze verify systemd/waycontrol.service` parsed the unit but reported its configured `%h/.local/bin/waycontrold` executable is absent because the service has not been installed. The service itself was not started under systemd.
- LAN binding is opt-in and must be paired with host firewall rules for a trusted network. The project does not configure firewall policy.
- `PROGRESS.md` records the complete test evidence, measured runtime data, and product work that remains before v0.1 acceptance.

## Review performed (2026-11-08)

A focused source review covered QR pairing generation/decoding, pairing endpoint validation, certificate pinning, and the CI workflow. The QR expiry calculation previously used unchecked `Long` subtraction. An attacker-controlled QR timestamp could overflow that subtraction and appear to be within the allowed lifetime. Android now uses checked subtraction and rejects overflow; a regression test reproduced the old behavior and passes with the fix. This bypassed only the client's expiry check: the QR contains no device credential, and the daemon still validates the live pairing code and requires local approval.

This was a focused review, not a full-project security assessment. Live device QR scanning and hostile-network testing remain outstanding.

## Review performed (2026-11-08)

The automation profile checkpoint was reviewed for typed operation dispatch, persisted-data validation, confirmation gating, and launch argument handling. Application launch accepts only a bounded desktop entry ID and executes `gtk-launch` with a single argument. The Arch package now declares GTK 3 as a runtime dependency. The profile actions do not provide shell execution.

The dependency advisory scan, secret scan, and protocol parser fuzz run are recorded above. Re-run the fuzzer with `cargo +nightly fuzz run parse_request_envelope -- -max_total_time=45 -max_len=65536`. The D-Bus dependency advisories are informational maintenance notices, not reported vulnerabilities. The broader independent security review and hostile-network testing remain outstanding.

## Review performed (2026-11-08)

A review of host command execution found that `Command::output()` captured stdout and stderr without a size limit. A noisy or misbehaving allowlisted utility could therefore consume daemon memory, and dropping a timed-out output future did not ensure the child process was killed. Both captured streams now have explicit limits, are drained concurrently, and the child is configured to terminate when the operation is cancelled. Regression tests verify stdout and stderr limit enforcement. Workspace tests and Clippy passed after the fix.

Report suspected vulnerabilities through the project maintainer. Do not expose a LAN-bound service on an untrusted network.

A follow-up source review checked the HTTP body limit, pairing state transitions, bearer authentication and rate limits, event-stream bounds/revocation, Android certificate pinning, and pairing QR validation. It found no additional confirmed issue. This is source-level review; it does not replace independent review or hostile-network testing.
