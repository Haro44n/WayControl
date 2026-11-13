package dev.waycontrol

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Test

class HomeDashboardSummaryTest {
    @Test
    fun homeActionsRequireAnAuthenticatedConnectedState() {
        assertEquals(true, homeActionsEnabled("Connected"))
        assertEquals(false, homeActionsEnabled("Disconnected"))
        assertEquals(false, homeActionsEnabled("Connecting"))
    }

    @Test
    fun automationRequiresBothStoredCredentialsAndALiveConnection() {
        assertEquals(true, automationActionsEnabled("Connected", hasToken = true))
        assertEquals(false, automationActionsEnabled("Disconnected", hasToken = true))
        assertEquals(false, automationActionsEnabled("Connected", hasToken = false))
    }

    @Test
    fun summaryShowsActualDesktopOsCompositorWorkspaceAppAndMeasuredLatency() {
        val desktop = JSONObject()
            .put("computer_name", "Cherlove Desktop")
            .put("os_name", "Arch Linux")
            .put("compositor", JSONObject().put("name", "Hyprland").put("version", "0.57.0"))
        val workspaces = JSONArray().put(JSONObject().put("id", "3").put("name", "web").put("active", true))
        val windows = JSONArray().put(JSONObject().put("focused", true).put("app_id", "firefox"))

        val summary = homeDashboardSummary(desktop, workspaces, windows, 12)

        assertEquals("Cherlove Desktop", summary.computerName)
        assertEquals("Arch Linux", summary.operatingSystem)
        assertEquals("Hyprland 0.57.0", summary.compositor)
        assertEquals("web", summary.activeWorkspace)
        assertEquals("firefox", summary.activeApplication)
        assertEquals("12 ms", summary.networkLatency)
    }

    @Test
    fun summaryNeverFabricatesUnavailableDesktopValues() {
        val summary = homeDashboardSummary(JSONObject(), JSONArray(), JSONArray(), null)

        assertEquals("Desktop", summary.computerName)
        assertEquals("Unavailable", summary.operatingSystem)
        assertEquals("Compositor not detected", summary.compositor)
        assertEquals("Unavailable", summary.activeWorkspace)
        assertEquals("Unavailable", summary.activeApplication)
        assertEquals("Unavailable", summary.networkLatency)
    }
}
