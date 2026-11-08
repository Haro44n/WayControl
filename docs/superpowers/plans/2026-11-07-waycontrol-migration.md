# WayControl Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rename HyprControl to WayControl and introduce a tested compositor backend/capability architecture without regressing Hyprland.

**Architecture:** A `waycontrol-compositor` crate owns compositor identity, detection, capability models, and the async compositor contract. The daemon composes a compositor adapter with compositor-independent system controls and telemetry. The Android client negotiates capabilities from authenticated desktop info and only exposes supported compositor operations.

**Tech Stack:** Rust workspace, Tokio, async-trait, Serde, Axum; Android Kotlin, Jetpack Compose, Gradle.

**Spec:** `docs/superpowers/specs/2026-11-07-waycontrol-migration-design.md`

## Global Constraints

- Preserve existing TLS pinning, Linux pairing approval, encrypted Android credential persistence, rate limits, and request validation.
- Keep operations typed; never expose arbitrary compositor commands.
- Do not advertise Sway, Niri, KDE KWin, or GNOME functionality before the adapter exists and is tested.
- Keep telemetry, networking, media, and host system controls compositor independent where possible.
- Maintain WayControl branding and tagline: “Your desktop. Your control.”

## Review Focus

- Hyprland detection when session variables are partial or inconsistent; test explicit precedence and safe unknown fallback.
- Unsupported compositor requests must not fall through to Hyprland or invoke a shell command; test capability rejection.
- Missing capability fields from older daemons must not cause the Android UI to expose controls; test conservative parsing.
- Window identifiers remain backend-owned opaque IDs and validation must not imply Hyprland hex addresses for other backends; keep compatibility and add backend-scoped validation.
- Renamed config/service paths must preserve access to existing paired-device state; provide an explicit migration or stable compatibility path.

---

### Task 1: Compositor identity and capability contract

**Files:**
- Create: `crates/waycontrol-compositor/Cargo.toml`
- Create: `crates/waycontrol-compositor/src/lib.rs`
- Modify: `Cargo.toml`
- Test: `crates/waycontrol-compositor/src/lib.rs`

**Interfaces:**
- Produces `CompositorKind`, `CompositorInfo`, `CompositorCapabilities`, `detect_compositor_from(...)`, and async `CompositorBackend` methods for workspace/window/monitor state and actions plus events.
- Capability fields: workspace listing/switching; window listing/focus/move/floating/fullscreen/close; monitor listing/power; event subscription.

- [ ] Add failing tests for each supported environment signature, precedence, unknown sessions, and zero capabilities for future compositor identities.
- [ ] Run the focused test and confirm expected missing API failure.
- [ ] Implement serializable models, deterministic environment detection, and the compositor trait.
- [ ] Run focused tests, then Rust formatting and Clippy.

### Task 2: WayControl rename and compatibility paths

**Files:** Cargo workspace/crate manifests and lockfile; `android/settings.gradle.kts`; `android/app/build.gradle.kts`; Android Kotlin packages and labels; README/PROGRESS/SECURITY/CI; systemd unit; Arch instructions; scripts; exported artifact names.

**Interfaces:** New binary `waycontrold`; new Android ID `dev.waycontrol`; new service `waycontrol.service`.

- [ ] Enumerate branded identifiers while preserving accurate `Hyprland` names.
- [ ] Rename crates, source package paths, service, app IDs, labels, commands, config directories, and artifact names.
- [ ] Preserve access to old config and credential data through a migration/fallback before creating new state.
- [ ] Run Rust and Android builds; search source/config/docs for stale product identifiers.

### Task 3: Extract and compose backend adapters

**Files:** create `crates/waycontrol-backend-hyprland`; move Hyprland adapter implementation out of HTTP server; modify `waycontrol-server` manifests/main/API.

**Interfaces:** `WaylandBackend` selects a compositor adapter while host system controls remain a separate service. Hyprland, Sway, Niri, and KWin workspace list/switch have adapters; GNOME and unknown sessions remain unsupported.

- [ ] Add regression tests for Hyprland adapter capability declarations and unsupported adapter behavior.
- [ ] Move existing Hyprland operations with no behavior changes; preserve event line limits and SSE authentication.
- [ ] Centralize capability enforcement so unsupported operations produce a stable `unsupported_operation` response.
- [ ] Run all Rust tests and live Hyprland loopback integration.

### Follow-up: add Sway adapter and named workspace IDs

**Files:** `waycontrol-backend-sway`, shared core/protocol workspace identifiers, Android workspace screens, daemon backend selection, README and installation notes.

**Interfaces:** Sway and Niri IPC identities pass through the common compositor contract as opaque IDs; numeric JSON IDs remain compatible with Hyprland clients. Each adapter exposes only capabilities implemented and observable through its IPC contract.

- [x] Add bounded Sway IPC framing, workspace/window/output reads, typed command actions, and event forwarding.
- [x] Add protocol fixture tests for named workspace IDs, window mapping, workspace counts, and safe command quoting.
- [ ] Run the adapter inside a live Sway session and verify advertised actions/events.
- [ ] Run Android build, emulator instrumentation, and Hyprland loopback regression after ID/UI changes.

### Follow-up: add Niri adapter

**Files:** `waycontrol-backend-niri`, daemon backend selection, compositor documentation.

- [x] Implement bounded newline JSON IPC for workspace/window/output queries, typed ID-based actions, and event forwarding.
- [x] Add fixtures for query mapping, actions, and event delivery.
- [ ] Verify the advertised Niri features inside a live Niri session.

### Task 4: Capability negotiation and Android adaptation

**Files:** compositor info handler and tests; Android `RemoteSnapshot`, API client, `MainActivity`, Compose tests.

**Interfaces:** `desktop.info` adds `compositor` metadata and granular capability booleans. Android treats absent/unknown values as unsupported.

- [ ] Add failing Rust tests for metadata and unsupported operation rejection.
- [ ] Add Kotlin tests for conservative parsing and destination/action visibility.
- [ ] Implement capability-based destination and action filtering while keeping common Home/System/Settings available.
- [ ] Run Rust and Android unit/instrumentation suites and builds.

### Task 5: CI, packaging, and status documentation

**Files:** `.github/workflows/rust.yml`, README, PROGRESS, SECURITY, Arch instructions, systemd unit, output filenames.

- [ ] Update CI for renamed Rust workspace and Android build/test checks where supported.
- [ ] Update installation/service/pairing instructions and package paths.
- [ ] Record implemented backends and unsupported detected sessions precisely.
- [ ] Run final Rust, Android, and real Hyprland integration checks; report remaining hardware-only validation honestly.
