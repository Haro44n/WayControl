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

- [ ] Write parser and calculation tests first; verify they fail before implementation.
- [ ] Implement typed parsing and source-specific errors.
- [ ] Run `cargo test --workspace` and `cargo fmt --all -- --check`.

### Task 2: Native Android shell

**Files:** `android/**`.

**Interfaces:**
- Compose app with Home, Workspaces, Windows, System, Settings destinations.
- Home shows only connection-required placeholders until a trusted live backend exists.

- [ ] Add project configuration and state-driven Compose navigation.
- [ ] Add presentation tests for the disconnected state where tooling permits.
- [ ] Attempt Gradle build only if Gradle and Android SDK are available; record otherwise.

### Task 3: Packaging and project documentation

**Files:** `README.md`, `PROGRESS.md`, `LICENSE`, `packaging/**`, `.github/workflows/**`.

- [ ] Document implemented behavior, security boundaries, installation, and known limitations.
- [ ] Add a user-level systemd unit that does not listen on the network.
- [ ] Add Rust CI and run locally available validation.

## Scope gaps tracked for later phases

Authenticated TLS pairing, Android network client, event streaming, window focus/actions, GPU/NVML, audio/MPRIS, power controls, automation, LAN discovery, Android lifecycle tests, and real device testing are not claimed by this foundation slice.
