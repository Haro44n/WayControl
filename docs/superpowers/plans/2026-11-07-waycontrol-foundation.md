# WayControl Foundation Implementation Plan

> **For agentic workers:** Use this plan task by task; each task is a self-contained deliverable.

**Goal:** Establish a buildable WayControl repository with a real Linux discovery/telemetry core and a native Android Compose shell.

**Architecture:** A Cargo workspace separates shared domain types, typed protocol models, Hyprland IPC, Linux telemetry, and the local diagnostic entrypoint. The Android module uses Compose and explicit connection state; it must not display fabricated host metrics. Network pairing and control remain gated until authenticated transport is implemented.

**Tech Stack:** Rust stable, Tokio, Serde, typed Hyprland IPC, `/proc` telemetry; Kotlin, Jetpack Compose, Material 3.

**Spec:** User-pasted WayControl task (attachment referenced in the conversation).

## Global Constraints

- Server: Arch Linux, Hyprland, Wayland.
- Client: Android, targeting Poco M5 initially.
- Backend language: Rust; Android language: Kotlin; UI framework: Jetpack Compose.
- KDE Connect is not a dependency and its configuration is not modified.
- The daemon must not expose an unauthenticated remote API.
- Never show fabricated system metrics or simulated production state.
- Dangerous operations remain disabled until validated and authorized server operations exist.

## Review Focus

- Missing or malformed Hyprland IPC responses must return errors rather than fabricated state; test parser rejection.
- First CPU sample has no delta baseline; test that it is unavailable rather than misleading.
- Android without an SDK must not be reported as build-verified; document the limitation.
- A missing Hyprland instance must leave the daemon usable for telemetry; test independent source failures.
- Remote control must not become reachable without authentication; this foundation binds no network listener.

---

### Task 1: Rust domain, Hyprland IPC, and Linux telemetry

**Files:** `Cargo.toml`, `crates/waycontrol-core/**`, `crates/waycontrol-hyprland/**`, `crates/waycontrol-telemetry/**`, `crates/waycontrol-server/**`.

**Interfaces:**
- Produces typed workspace/window models, version-1 allowlisted protocol envelopes, a Hyprland IPC client, and telemetry snapshots from Linux procfs.
- Server command `waycontrold inspect` emits available real host data as JSON and reports unavailable sources explicitly.

- [x] Add parser and calculation regression tests for valid, malformed, unavailable, and first-sample cases.
- [x] Implement typed parsing and source-specific errors.
- [x] Run `cargo test --workspace` and `cargo fmt --all -- --check`.

### Task 2: Native Android shell

**Files:** `android/**`.

**Interfaces:**
- Compose app with Home, Workspaces, Windows, System, Settings destinations.
- Home shows only connection-required placeholders until a trusted live backend exists.

- [x] Add project configuration and state-driven Compose navigation.
- [x] Add presentation tests for disconnected, capability-limited, and live-data states.
- [x] Build the Android app and run JVM plus Android 16 emulator instrumentation tests.

### Task 3: Packaging and project documentation

**Files:** `README.md`, `PROGRESS.md`, `LICENSE`, `packaging/**`, `.github/workflows/**`.

- [x] Document implemented behavior, security boundaries, installation, and known limitations.
- [x] Add a user-level systemd unit that binds to loopback by default.
- [x] Add Rust, Android, and live-compositor CI and run validation.

## Later phases completed

- [x] Authenticated TLS pairing, encrypted Android credentials, local Linux approval, and revocation.
- [x] Android network client, compositor capability negotiation, workspace/window screens, and lifecycle reconnection.
- [x] Live compositor events plus independent telemetry, audio/MPRIS, GPU/NVML, power, and automation controls.
- [x] LAN discovery and QR pairing entry, with certificate pinning and explicit approval preserved.
- [x] Rust/Android builds, Android 16 instrumentation, Hyprland loopback integration, package build, and systemd sandbox integration.

Physical Poco M5 behavior, QR camera scanning, discovery from a phone over physical Wi-Fi, and users' own Sway/Niri/Plasma/GNOME sessions remain open; see `PROGRESS.md` for current evidence and limits.
