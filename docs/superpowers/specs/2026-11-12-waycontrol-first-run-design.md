# WayControl First-Run Connection Design

## Intent

Make a fresh WayControl install guide a user from first open to a trusted desktop connection without requiring them to discover Settings first. This implements the original project request's welcome → server discovery/manual entry → secure pairing → Linux approval → dashboard flow.

## User flow

- A fresh install with no saved trusted connection opens a welcome/setup screen. It starts one bounded LAN discovery scan automatically and displays the same server list and status used in Settings.
- Selecting a discovered host fills only its HTTPS endpoint. Discovery never supplies or trusts a certificate fingerprint.
- The user can instead enter the endpoint, scan a pairing QR code, or manually enter the endpoint, SHA-256 fingerprint, and short-lived code. Existing TLS pin validation remains enabled.
- Pairing stays pending until the daemon confirms Linux-side approval. Only then are credentials encrypted and saved, onboarding is marked complete, and the app opens Home.
- The user can skip setup and use the disconnected app; the completed-onboarding preference prevents the welcome screen from returning on later launches. Pairing remains available from Settings.
- An installation with saved credentials skips welcome, restores the saved endpoint/pin/token, and follows the existing reconnect behavior.

## Constraints

- Reuse the existing Compose visual system and current Settings pairing/discovery behavior; do not add dependencies or background services.
- Discovery remains bounded and untrusted. Pairing still requires a trusted fingerprint and explicit Linux approval.
- Do not show Home as connected before server confirmation. Do not discard existing encrypted credentials or change reconnect behavior.
- Keep manual setup available when LAN discovery or Google Play QR scanning is unavailable.

## Verification

- Unit tests cover the welcome route for fresh, previously completed, and credential-restoration cases.
- Compose instrumentation verifies first-run welcome, discovery status, skip-to-dashboard, pairing entry availability, and direct dashboard transition after a confirmed connection.
- Existing secure pairing, QR validation, connection restore/reconnect, capability, lint/build, and Android 16 instrumentation suites remain green.

## Implementation call

The existing Settings form remains the source of truth for endpoint, fingerprint, pairing code, QR, discovery results, and pairing actions. The first-run screen reuses that form in a welcome presentation and hides bottom navigation until the user skips or pairing succeeds.
