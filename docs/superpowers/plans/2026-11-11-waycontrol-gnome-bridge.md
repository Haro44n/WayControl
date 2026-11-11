# WayControl GNOME bridge implementation plan

Spec: `docs/superpowers/specs/2026-11-11-waycontrol-gnome-bridge-design.md`
Base: `06ff04c905bc14e9baa7b9026afac88849925029`

## Goal
Implement an optional GNOME Shell session-bus extension and Rust adapter with startup capability negotiation, bounded IDs and payloads, correct nullable floating state, event forwarding, packaging, and user-controlled manual enablement. Push each tested checkpoint to `origin/main` as requested.

## Global constraints
- GNOME Wayland only, with the exact GNOME desktop identity and `XDG_SESSION_TYPE=wayland`.
- Fixed versioned D-Bus interface, no subprocess, eval, or arbitrary command dispatch.
- Advertise compositor capabilities only after successful D-Bus version negotiation.
- Never auto-install/enable the extension, alter settings, or restart GNOME Shell.
- Existing non-GNOME adapter behavior must remain intact.

## Task 1 — Represent unknown floating state
- Change shared `Window.floating` to `Option<bool>` with absent/unknown encoded as JSON null.
- Update existing backend constructors to `Some(value)` and update serialization/API tests.
- TDD: add failing tests proving known adapter values stay booleans and unknown is null, then implement.
- Expected: targeted core/backend/server tests pass; existing adapter output remains stable.
- Checkpoint commit/push: `feat(core): represent unknown window floating state`.

## Task 2 — Rust GNOME D-Bus adapter and startup discovery
- Add `waycontrol-backend-gnome` crate; define fixed v1 object/interface contract and strict bounded JSON conversion for workspaces/windows/events.
- Use zbus session connection and startup `GetVersion`; exact version 1 enables only workspace/window/focus/move/fullscreen/close/events capabilities. Missing service, bad version or non-Wayland session produces unsupported zero capabilities.
- Resolve and validate opaque IDs before every mutation; fail closed on bus errors. Forward bounded change signals through UnixStream.
- Integrate async GNOME detection into daemon startup without blocking Tokio and preserve existing synchronous non-GNOME detection behavior.
- TDD: mock D-Bus tests for absent, incompatible and supported interface, typed calls, malformed/oversized data, stale IDs, and event stream closure.
- Expected: targeted adapter/server tests pass under `dbus-run-session`; all existing backend paths unchanged.
- Checkpoint commit/push: `feat(gnome): add negotiated D-Bus compositor adapter`.

## Task 3 — GNOME Shell extension, packaging and docs
- Add GNOME Shell extension v1 exporting fixed typed D-Bus methods/signals and using live Mutter state for workspace/window operations.
- Bound/clean titles and IDs; do not expose floating, monitor-power, or lock-screen support.
- Add extension validation and nested GNOME integration CI where available; tests must run isolated and must not mutate host state.
- Package extension source as optional files only; document manual installation, enable/disable, supported/tested shell versions, session authority, and unavailable behavior when absent.
- TDD: write failing extension/packaging checks first, then implement.
- Expected: JS syntax/static tests and packaging checks pass; GNOME-specific integration job reports honest availability or is explicitly skipped if runner packages cannot support nested Shell.
- Checkpoint commit/push: `feat(gnome): ship opt-in GNOME Shell bridge`.

## Final review
- Run formatting, clippy, complete Rust workspace tests, release build, extension tests, package checks, and Android CI status.
- Review full diff for user-facing overclaims, auto-enable side effects, unbounded D-Bus data, and API compatibility.
- Record known limitations in the progress ledger and `PROGRESS.md`.
