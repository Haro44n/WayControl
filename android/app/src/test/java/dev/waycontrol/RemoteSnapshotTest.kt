package dev.waycontrol

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test

class RemoteSnapshotTest {
    @Test
    fun clearingDisconnectedSnapshotRemovesPreviouslyDisplayedDesktopData() {
        val previous = RemoteSnapshot(
            desktop = JSONObject().put("cpu_usage_percent", 37.5),
            workspaces = JSONArray().put(JSONObject().put("id", 1)),
            windows = JSONArray().put(JSONObject().put("address", "0x123")),
            monitors = JSONArray().put(JSONObject().put("name", "DP-1")),
        )

        val disconnected = previous.cleared()

        assertFalse(disconnected.desktop.has("cpu_usage_percent"))
        assertEquals(0, disconnected.workspaces.length())
        assertEquals(0, disconnected.windows.length())
        assertEquals(0, disconnected.monitors.length())
    }
}
