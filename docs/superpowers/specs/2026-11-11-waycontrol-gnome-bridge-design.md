# WayControl GNOME bridge design

## Intent

Add real, explicitly opt-in GNOME workspace and window control to WayControl while keeping GNOME unsupported when the companion integration is absent. Do not claim that stock GNOME exposes the compositor operations WayControl needs.

## Evidence and constraints

- GNOME's `org.freedesktop.portal.RemoteDesktop` mediates user-approved keyboard, pointer, or touchscreen input. It does not expose WayControl's workspace and window inventory/actions, so it is not a substitute for the compositor backend.
- Mutter's `Meta.Workspace` and `Meta.Window` APIs expose workspace/window state and operations to code running inside GNOME Shell. GNOME Shell extensions can export a typed D-Bus object from GJS.
- The initial bridge targets GNOME Wayland sessions only. GNOME Shell on X11 remains unsupported for this backend.
- GNOME Shell extensions execute in the compositor process and use APIs whose compatibility must be tested against supported Shell versions. WayControl must not silently install or enable one.
- The current `CompositorBackend::capabilities()` is synchronous, while session-bus discovery is asynchronous. Adapter initialization/capability negotiation must resolve this cleanly before implementation.

Primary references: [XDG RemoteDesktop portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html), [Mutter `Meta.Window`](https://gnome.pages.gitlab.gnome.org/mutter/meta/class.Window.html), [Mutter `Meta.Workspace`](https://gnome.pages.gitlab.gnome.org/mutter/meta/class.Workspace.html), [GJS D-Bus guide](https://gjs.guide/guides/gio/dbus.html), and [GNOME Shell extension testing guide](https://gjs.guide/extensions/development/creating.html).

## Proposed shape

Ship a small optional WayControl GNOME Shell extension alongside a Rust `waycontrol-backend-gnome` adapter. The extension exposes only a versioned, fixed D-Bus interface on the user's session bus. The Rust adapter calls that interface directly through `zbus`; it does not spawn `gdbus` or pass arbitrary JavaScript to GNOME Shell.

The extension will implement only operations that the existing protocol and `CompositorBackend` can represent safely:

- List workspaces and windows, including active state, workspace assignment, monitor assignment, fullscreen state, and a bounded opaque window ID. Do not invent floating/tiled state where Mutter provides no equivalent.
- Switch to an existing workspace; focus, close, move, or toggle fullscreen on a currently listed window after resolving the opaque ID against the current Shell state. Floating-state toggling remains unsupported because Mutter does not expose an equivalent operation in the shared backend contract.
- Emit bounded workspace/window change events on a local D-Bus signal which the Rust adapter forwards to the existing event stream.

The first version will not add new remote input/desktop-capture operations, arbitrary Shell evaluation, extension management, per-monitor power, or lock-screen actions. Missing extension, missing methods, unsupported Shell version, or a closed D-Bus name results in zero GNOME compositor capabilities and an explicit unavailable state. Advertised capabilities are derived from successful extension/version discovery during daemon startup; loss of the service makes calls fail closed and will be reflected after daemon restart. Runtime renegotiation can be added only if the session-bus owner-change behavior is covered by tests.

GNOME adapter selection requires both a GNOME desktop identity and `XDG_SESSION_TYPE=wayland`; a GNOME label alone does not select the bridge. The extension supports fullscreen toggling, while the floating toggle capability stays false.

## Installation and user control

- Put extension source and metadata in the repository and package it as an optional GNOME integration file set.
- Installation may place files in the documented per-user or system extension directory, but must not enable the extension, edit GNOME settings, restart Shell, or alter portal/KDE Connect/Hyprland configuration.
- Document the supported GNOME Shell versions, manual enable/disable steps, and the fact that the extension runs inside GNOME Shell with the user's session authority.
- Do not mark GNOME controls available unless the extension is active and its interface version is supported.

## Security boundaries

- The extension has a narrow method allowlist; there is no `eval`, shell dispatch string, arbitrary method name, file access, or network operation.
- Revalidate bounded IDs and action arguments in the extension and Rust adapter. Resolve window IDs against live Shell objects immediately before a mutation.
- Protect daemon network calls with the existing TLS pinning, pairing approval, bearer authentication, and request limits. D-Bus is only the local compositor boundary.
- Treat GNOME window titles as sensitive desktop data, as in the existing authenticated window-list API.

## Verification

- Unit tests: capability negotiation for absent, incompatible, and supported extension interfaces; typed method serialization; stale/invalid IDs; mapping of workspaces/windows; event bounds and closure.
- Integration tests: run the extension and adapter in an isolated nested GNOME Shell session on CI, verify read/switch/event/focus/move/close behavior using temporary test windows, and restore the original active workspace. Never test destructive host actions.
- Packaging tests: verify extension files are included but not auto-enabled, and that non-GNOME installs remain unaffected.
- Keep real user-session testing distinct from the nested CI session and do not claim physical Plasma or GNOME validation from virtual-compositor jobs.

## Decisions left to the implementer

- Choose an interface versioning and GNOME Shell version policy based on the GNOME versions available in CI and the current upstream Mutter API.
- Choose a startup initialization shape that preserves synchronous capability reporting without blocking the Tokio runtime.
- Keep the extension's public D-Bus methods and signals documented with exact signatures and bounded payload rules.

Builder review confirmed these scope decisions: Wayland-only activation, fullscreen toggle support, and no floating toggle. The existing project objective supplies the platform scope; Mutter exposes the fullscreen operation, while floating has no equivalent shared operation.

## Approval boundary

This design adds an optional in-process GNOME Shell extension because the required workspace/window APIs are otherwise unavailable. Do not implement or package that extension until this design is approved.
