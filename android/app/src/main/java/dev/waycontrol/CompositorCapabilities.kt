package dev.waycontrol

import org.json.JSONObject

internal data class CompositorCapabilities(
    val workspaces: Boolean = false,
    val switchWorkspace: Boolean = false,
    val windows: Boolean = false,
    val focusWindow: Boolean = false,
    val moveWindow: Boolean = false,
    val toggleFloating: Boolean = false,
    val toggleFullscreen: Boolean = false,
    val closeWindow: Boolean = false,
    val monitors: Boolean = false,
    val monitorPower: Boolean = false,
    val events: Boolean = false,
    val lockScreen: Boolean = false,
) {
    companion object {
        fun fromDesktop(desktop: JSONObject): CompositorCapabilities {
            val values = desktop.optJSONObject("capabilities") ?: return CompositorCapabilities()
            return CompositorCapabilities(
                workspaces = values.optBoolean("workspaces"),
                switchWorkspace = values.optBoolean("switch_workspace"),
                windows = values.optBoolean("windows"),
                focusWindow = values.optBoolean("focus_window"),
                moveWindow = values.optBoolean("move_window"),
                toggleFloating = values.optBoolean("toggle_floating"),
                toggleFullscreen = values.optBoolean("toggle_fullscreen"),
                closeWindow = values.optBoolean("close_window"),
                monitors = values.optBoolean("monitors"),
                monitorPower = values.optBoolean("monitor_power"),
                events = values.optBoolean("events"),
                lockScreen = values.optBoolean("lock_screen"),
            )
        }
    }
}

internal data class CompositorFetchPlan(
    val workspaces: Boolean,
    val windows: Boolean,
    val monitors: Boolean,
)

internal fun compositorFetchPlan(capabilities: CompositorCapabilities) = CompositorFetchPlan(
    workspaces = capabilities.workspaces,
    windows = capabilities.windows,
    monitors = capabilities.monitors,
)
