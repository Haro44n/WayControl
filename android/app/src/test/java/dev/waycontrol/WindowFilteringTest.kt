package dev.waycontrol

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Test

class WindowFilteringTest {
    @Test
    fun searchMatchesTitleApplicationWorkspaceAndMonitorAndGroupsResults() {
        val windows = JSONArray()
            .put(JSONObject().put("address", "0x1").put("title", "Terminal").put("app_id", "kitty").put("workspace_id", 1).put("monitor", "DP-1"))
            .put(JSONObject().put("address", "0x2").put("title", "WayControl source").put("app_id", "code").put("workspace_id", 2).put("monitor", "HDMI-A-1"))
        val workspaces = JSONArray()
            .put(JSONObject().put("id", 1).put("name", "1: web"))
            .put(JSONObject().put("id", 2).put("name", "2: dev"))

        val titleSearch = groupFilteredWindows(windows, workspaces, "waycontrol", WindowGrouping.WORKSPACE)
        assertEquals(listOf("2: dev"), titleSearch.map { it.label })
        assertEquals("0x2", titleSearch.single().windows.single().optString("address"))

        val appSearch = groupFilteredWindows(windows, workspaces, "kitty", WindowGrouping.WORKSPACE)
        assertEquals(listOf("1: web"), appSearch.map { it.label })

        val monitorSearch = groupFilteredWindows(windows, workspaces, "hdmi-a-1", WindowGrouping.MONITOR)
        assertEquals(listOf("HDMI-A-1"), monitorSearch.map { it.label })

        val allByMonitor = groupFilteredWindows(windows, workspaces, "", WindowGrouping.MONITOR)
        assertEquals(listOf("DP-1", "HDMI-A-1"), allByMonitor.map { it.label })
    }
}
