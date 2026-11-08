package dev.waycontrol

import org.json.JSONArray
import org.json.JSONObject

internal data class HomeDashboardSummary(
    val computerName: String,
    val operatingSystem: String,
    val compositor: String,
    val activeWorkspace: String,
    val activeApplication: String,
    val networkLatency: String,
)

internal fun homeDashboardSummary(
    desktop: JSONObject,
    workspaces: JSONArray,
    windows: JSONArray,
    networkLatencyMs: Long?,
): HomeDashboardSummary {
    val compositorInfo = desktop.optJSONObject("compositor")
    val compositorName = compositorInfo?.optString("name")?.takeIf(String::isNotBlank)
        ?: "Compositor not detected"
    val compositorVersion = compositorInfo?.optString("version")?.takeIf(String::isNotBlank)
    val activeWorkspace = (0 until workspaces.length())
        .mapNotNull(workspaces::optJSONObject)
        .firstOrNull { it.optBoolean("active") }
        ?.let { it.optString("name").takeIf(String::isNotBlank) ?: it.optString("id") }
        ?.takeIf(String::isNotBlank)
        ?: "Unavailable"
    val activeApplication = (0 until windows.length())
        .mapNotNull(windows::optJSONObject)
        .firstOrNull { it.optBoolean("focused") }
        ?.let { it.optString("app_id").takeIf(String::isNotBlank) ?: it.optString("title") }
        ?.takeIf(String::isNotBlank)
        ?: "Unavailable"

    return HomeDashboardSummary(
        computerName = desktop.optString("computer_name").takeIf(String::isNotBlank) ?: "Desktop",
        operatingSystem = desktop.optString("os_name").takeIf(String::isNotBlank) ?: "Unavailable",
        compositor = if (compositorVersion == null) compositorName else "$compositorName $compositorVersion",
        activeWorkspace = activeWorkspace,
        activeApplication = activeApplication,
        networkLatency = networkLatencyMs?.takeIf { it >= 0 }?.let { "$it ms" } ?: "Unavailable",
    )
}
