# WayControl Compositor Migration Design

## Intent

Rename WayControl to **WayControl** and evolve the existing Android companion and Rust daemon so compositor-specific operations pass through a modular backend contract. The immediate priority is preserving the tested Hyprland experience while making capability boundaries explicit for future Wayland compositors.

## Goals

- Consistently rename application/package/crate/service/docs identifiers to WayControl while retaining accurate Hyprland protocol and implementation names.
- Keep existing TLS pinning, pairing approval, credential persistence, request validation, rate limits, telemetry, media and system controls intact.
- Define compositor identity and granular capabilities in a Rust crate independent of HTTP and Android.
- Keep Hyprland workspace/window/monitor operations behind a `CompositorBackend` adapter.
- Detect Hyprland, Sway, Niri, KDE KWin, GNOME, and unknown sessions from session environment signals.
- Expose detected compositor and supported operations to Android; do not advertise or dispatch unsupported operations as available.
- Preserve compatibility for Hyprland users while future adapters are built and tested.

## Non-goals for this migration stage

- Claiming live-tested Sway or Niri support before exercising those adapters inside matching sessions; KWin workspace list/switch support is implemented but awaits live-session testing, while KWin window controls and GNOME compositor controls remain unsupported.
- Replacing the existing HTTPS protocol, pairing flow, telemetry collectors, or Android design.
- Adding arbitrary command execution, compositor shell passthrough, or privileged sudo configuration.

## Architecture decisions

- Add a focused `waycontrol-compositor` crate for serializable compositor identity/capability models, detection, and the async compositor API.
- Inject host system controls separately from compositor workspace/window/monitor operations. The existing allowlisted power/audio/media commands remain host services and do not depend on compositor identity; screen locking is a compositor capability because the current implementation uses `hyprlock`.
- Represent adapters as `WaylandBackend::Hyprland`, `WaylandBackend::Sway`, `WaylandBackend::Niri`, or an explicitly unsupported detected backend. Unsupported adapters report all compositor capabilities as false and return an explicit unsupported error if called. Sway and Niri capabilities are fixture-tested but still need live session checks.
- Keep common `/proc` and NVML telemetry outside compositor adapters.
- Extend the authenticated desktop information response with additive `compositor` and `capabilities` fields. Existing telemetry fields stay unchanged.
- Android filters compositor-specific destinations/actions using the negotiated capabilities; Home, System, and Settings remain available independently.
- Do not change the API protocol version for additive response fields. Older clients ignore fields; the new client treats absent capability data as unavailable.

## Constraints

- Do not discard or rewrite tested Hyprland behavior unnecessarily.
- Keep all remote operations typed and allowlisted.
- Keep unavailable telemetry represented as unavailable, not fabricated values.
- Preserve data, security, and reconnect behavior while changing names and backend wiring.
- The current checkout has no `.git` metadata, so changes cannot be reviewed through `git diff` unless repository metadata is restored. Use explicit file inspection and test/build evidence.

## Success evidence

- Rust unit tests cover detection precedence, unknown sessions, and zero-capability unsupported compositors.
- Sway adapter tests exercise IPC framing, named workspace IDs, safe workspace commands, and tree-to-window mapping using local protocol fixtures.
- Existing Hyprland IPC, server, security, and live loopback tests still pass.
- API tests show detected identity/capabilities and reject unsupported operations without reaching an adapter.
- Android tests/build pass and UI selection/actions follow the advertised capabilities.
- Rust formatting, Clippy, release build, Android unit/lint/debug/release build pass.
- Documentation, CI, systemd service, Arch instructions, executable/package names, and exported APK names use WayControl consistently.
- No documentation claims universal compositor support.
