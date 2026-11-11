import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const BUS_NAME = 'org.waycontrol.GnomeShell';
const OBJECT_PATH = '/org/waycontrol/GnomeShell';
const MAX_TEXT_CHARS = 112;
const MAX_WORKSPACES = 256;
const MAX_WINDOWS = 128;
const WORKSPACE_ID_RE = /^gnome:(0|[1-9][0-9]{0,19})$/;
const WINDOW_ID_RE = /^gnome:(0|[1-9][0-9]{0,19})$/;

const INTERFACE_XML = `
<node>
  <interface name="org.waycontrol.GnomeShell1">
    <method name="GetVersion">
      <arg type="u" direction="out" name="version"/>
    </method>
    <method name="ListWorkspaces">
      <arg type="s" direction="out" name="json"/>
    </method>
    <method name="ListWindows">
      <arg type="s" direction="out" name="json"/>
    </method>
    <method name="SwitchWorkspace">
      <arg type="s" direction="in" name="workspace_id"/>
      <arg type="b" direction="out" name="accepted"/>
    </method>
    <method name="FocusWindow">
      <arg type="s" direction="in" name="window_id"/>
      <arg type="b" direction="out" name="accepted"/>
    </method>
    <method name="MoveWindow">
      <arg type="s" direction="in" name="window_id"/>
      <arg type="s" direction="in" name="workspace_id"/>
      <arg type="b" direction="out" name="accepted"/>
    </method>
    <method name="ToggleFullscreen">
      <arg type="s" direction="in" name="window_id"/>
      <arg type="b" direction="out" name="accepted"/>
    </method>
    <method name="CloseWindow">
      <arg type="s" direction="in" name="window_id"/>
      <arg type="b" direction="out" name="accepted"/>
    </method>
    <signal name="Changed">
      <arg type="s" name="event"/>
    </signal>
  </interface>
</node>`;

function boundedText(value) {
    const text = String(value ?? '').replace(/[\u0000-\u001f\u007f-\u009f]/g, '');
    return Array.from(text).slice(0, MAX_TEXT_CHARS).join('');
}

function isExposedWindow(window) {
    const type = window.get_window_type();
    return type === Meta.WindowType.NORMAL || type === Meta.WindowType.DIALOG;
}

export default class WayControlGnomeBridge extends Extension {
    enable() {
        this._busOwnerId = Gio.bus_own_name(
            Gio.BusType.SESSION,
            BUS_NAME,
            Gio.BusNameOwnerFlags.NONE,
            connection => {
                this._exported = Gio.DBusExportedObject.wrapJSObject(INTERFACE_XML, this);
                this._exported.export(connection, OBJECT_PATH);
            },
            () => {},
            () => {
                this._exported?.unexport();
                this._exported = null;
            });

        this._workspaceIds = new WeakMap();
        this._nextWorkspaceId = 1;
        this._windowSignals = new Map();
        this._displaySignalIds = [
            this._connectOptional(global.display, 'window-created', (_display, window) => {
                this._watchWindow(window);
                this._emitChanged('windows-changed');
            }),
            this._connectOptional(global.display, 'window-removed', () => this._emitChanged('windows-changed')),
            this._connectOptional(global.display, 'notify::focus-window', () => this._emitChanged('windows-changed')),
        ];

        const manager = global.workspace_manager;
        this._workspaceSignalIds = [
            this._connectOptional(manager, 'active-workspace-changed', () => this._emitChanged('workspace-changed')),
            this._connectOptional(manager, 'workspace-added', () => this._emitChanged('workspace-changed')),
            this._connectOptional(manager, 'workspace-removed', () => this._emitChanged('workspace-changed')),
            this._connectOptional(manager, 'workspaces-reordered', () => this._emitChanged('workspace-changed')),
        ];
        this._windows();
    }

    disable() {
        for (const signalId of this._displaySignalIds ?? []) {
            if (signalId)
                global.display.disconnect(signalId);
        }
        for (const signalId of this._workspaceSignalIds ?? []) {
            if (signalId)
                global.workspace_manager.disconnect(signalId);
        }
        for (const signals of (this._windowSignals ?? new Map()).values()) {
            for (const [window, signalId] of signals)
                window.disconnect(signalId);
        }
        this._windowSignals?.clear();
        this._exported?.unexport();
        this._exported = null;
        if (this._busOwnerId !== undefined) {
            Gio.bus_unown_name(this._busOwnerId);
            this._busOwnerId = undefined;
        }
    }

    GetVersion() {
        return 1;
    }

    ListWorkspaces() {
        const workspaces = global.workspace_manager.get_workspaces();
        const active = global.workspace_manager.get_active_workspace();
        return JSON.stringify(workspaces.slice(0, MAX_WORKSPACES).map(workspace => ({
            id: this._workspaceId(workspace),
            name: `Workspace ${workspace.index() + 1}`,
            monitor: null,
            window_count: workspace.list_windows().filter(isExposedWindow).length,
            active: workspace === active,
        })));
    }

    ListWindows() {
        return JSON.stringify(this._windows());
    }

    SwitchWorkspace(id) {
        if (!WORKSPACE_ID_RE.test(id))
            return false;
        const workspace = this._workspaceFor(id);
        if (!workspace)
            return false;
        workspace.activate(global.get_current_time());
        return true;
    }

    FocusWindow(id) {
        const window = this._windowFor(id);
        if (!window)
            return false;
        window.activate(global.get_current_time());
        return true;
    }

    MoveWindow(id, workspaceId) {
        if (!WORKSPACE_ID_RE.test(workspaceId))
            return false;
        const window = this._windowFor(id);
        const workspace = this._workspaceFor(workspaceId);
        if (!window || !workspace)
            return false;
        window.change_workspace(workspace);
        return true;
    }

    ToggleFullscreen(id) {
        const window = this._windowFor(id);
        if (!window)
            return false;
        if (window.is_fullscreen())
            window.unmake_fullscreen();
        else
            window.make_fullscreen();
        return true;
    }

    CloseWindow(id) {
        const window = this._windowFor(id);
        if (!window)
            return false;
        window.delete(global.get_current_time());
        return true;
    }

    _workspaceId(workspace) {
        let id = this._workspaceIds.get(workspace);
        if (!id) {
            id = `gnome:${this._nextWorkspaceId++}`;
            this._workspaceIds.set(workspace, id);
        }
        return id;
    }

    _workspaceFor(id) {
        if (!WORKSPACE_ID_RE.test(id))
            return null;
        return global.workspace_manager.get_workspaces()
            .find(workspace => this._workspaceId(workspace) === id) ?? null;
    }

    _windows() {
        const windows = [];
        for (const actor of global.get_window_actors()) {
            const window = actor.meta_window;
            if (!window || !isExposedWindow(window))
                continue;
            this._watchWindow(window);
            const stableSequence = window.get_stable_sequence();
            if (!Number.isSafeInteger(stableSequence) || stableSequence < 0)
                continue;
            const workspace = window.get_workspace();
            windows.push({
                id: `gnome:${stableSequence}`,
                title: boundedText(window.get_title()),
                app_id: boundedText(window.get_sandboxed_app_id?.() || window.get_wm_class() || ''),
                workspace_id: workspace ? this._workspaceId(workspace) : null,
                monitor: String(window.get_monitor()),
                floating: null,
                fullscreen: window.is_fullscreen(),
                focused: window.has_focus(),
            });
            if (windows.length >= MAX_WINDOWS)
                break;
        }
        return windows;
    }

    _windowFor(id) {
        if (!WINDOW_ID_RE.test(id))
            return null;
        return global.get_window_actors()
            .map(actor => actor.meta_window)
            .find(window => window && isExposedWindow(window) && `gnome:${window.get_stable_sequence()}` === id) ?? null;
    }

    _watchWindow(window) {
        if (!window || !isExposedWindow(window))
            return;
        const key = window.get_stable_sequence();
        if (!Number.isSafeInteger(key) || this._windowSignals.has(key))
            return;
        const signals = [];
        for (const signal of ['notify::title', 'notify::wm-class', 'notify::fullscreen',
            'focus', 'workspace-changed', 'position-changed', 'size-changed',
            'highest-scale-monitor-changed']) {
            const signalId = this._connectOptional(window, signal, () => this._emitChanged('windows-changed'));
            if (signalId)
                signals.push([window, signalId]);
        }
        const unmanagedId = this._connectOptional(window, 'unmanaged', () => {
            this._forgetWindow(key);
            this._emitChanged('windows-changed');
        });
        if (unmanagedId)
            signals.push([window, unmanagedId]);
        this._windowSignals.set(key, signals);
    }

    _forgetWindow(key) {
        for (const [window, signalId] of this._windowSignals.get(key) ?? [])
            window.disconnect(signalId);
        this._windowSignals.delete(key);
    }

    _connectOptional(object, signal, callback) {
        try {
            return object.connect(signal, callback);
        } catch (_error) {
            return 0;
        }
    }

    _emitChanged(event) {
        if (!this._exported)
            return;
        this._exported.emit_signal('Changed', new GLib.Variant('(s)', [event]));
    }
}
