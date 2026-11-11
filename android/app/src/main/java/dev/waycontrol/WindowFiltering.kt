package dev.waycontrol

import org.json.JSONArray
import org.json.JSONObject
import java.util.Locale

internal enum class WindowGrouping { WORKSPACE, MONITOR }

internal data class WindowGroup(val label: String, val windows: List<JSONObject>)

internal fun groupFilteredWindows(
    windows: JSONArray,
    workspaces: JSONArray,
    query: String,
    grouping: WindowGrouping,
): List<WindowGroup> {
    val workspaceNames = buildMap {
        for (index in 0 until workspaces.length()) {
            val workspace = workspaces.optJSONObject(index) ?: continue
            val id = jsonText(workspace, "id")
            val name = jsonText(workspace, "name")
            if (id.isNotEmpty() && name.isNotEmpty()) put(id, name)
        }
    }
    val normalizedQuery = query.trim().lowercase(Locale.ROOT)
    val grouped = mutableMapOf<String, MutableList<JSONObject>>()
    for (index in 0 until windows.length()) {
        val window = windows.optJSONObject(index) ?: continue
        val appId = jsonText(window, "app_id")
        val title = jsonText(window, "title")
        val workspaceId = jsonText(window, "workspace_id")
        val workspace = workspaceNames[workspaceId] ?: workspaceId.ifEmpty { "No workspace" }
        val monitor = jsonText(window, "monitor").ifEmpty { "Unknown monitor" }
        val searchable = listOf(title, appId, workspaceId, workspace, monitor)
            .joinToString(" ")
            .lowercase(Locale.ROOT)
        if (normalizedQuery.isNotEmpty() && !searchable.contains(normalizedQuery)) continue
        val group = when (grouping) {
            WindowGrouping.WORKSPACE -> workspace
            WindowGrouping.MONITOR -> monitor
        }
        grouped.getOrPut(group) { mutableListOf() }.add(window)
    }
    return grouped.entries
        .sortedBy { it.key.lowercase(Locale.ROOT) }
        .map { (label, entries) -> WindowGroup(label, entries.toList()) }
}

private fun jsonText(value: JSONObject, key: String): String =
    value.optString(key).takeIf { it.isNotBlank() && it != "null" } ?: ""
