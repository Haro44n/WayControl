package dev.waycontrol

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class CompositorCapabilitiesTest {
    @Test
    fun missingOrUnknownCapabilityFieldsAreConservativelyUnavailable() {
        val parsed = CompositorCapabilities.fromDesktop(JSONObject())
        assertFalse(parsed.workspaces)
        assertFalse(parsed.windows)
        assertFalse(parsed.monitorPower)
        assertFalse(parsed.lockScreen)
        assertFalse(parsed.togglePinned)
        assertFalse(parsed.moveWindowToMonitor)
    }

    @Test
    fun readsCapabilitiesNegotiatedByTheDaemon() {
        val desktop = JSONObject("""
            {"capabilities":{"workspaces":true,"windows":true,"toggle_pinned":true,"move_window_to_monitor":true,"monitor_power":false,"lock_screen":true}}
        """.trimIndent())
        val parsed = CompositorCapabilities.fromDesktop(desktop)
        assertTrue(parsed.workspaces)
        assertTrue(parsed.windows)
        assertFalse(parsed.monitorPower)
        assertTrue(parsed.lockScreen)
        assertTrue(parsed.togglePinned)
        assertTrue(parsed.moveWindowToMonitor)
    }

    @Test
    fun kwinWorkspaceCapabilitiesShowWorkspaceNavigationAndEnableEvents() {
        val desktop = JSONObject("""
            {"compositor":{"id":"kde_kwin"},"capabilities":{"workspaces":true,"switch_workspace":true,"events":true}}
        """.trimIndent())
        val parsed = CompositorCapabilities.fromDesktop(desktop)
        assertTrue(parsed.workspaces)
        assertTrue(parsed.switchWorkspace)
        assertTrue(parsed.events)
        assertFalse(parsed.windows)
        assertFalse(parsed.monitors)
        assertEquals(
            listOf(Destination.Home, Destination.Workspaces, Destination.System, Destination.Automation, Destination.Settings),
            visibleDestinations(parsed),
        )
        assertEquals(
            CompositorFetchPlan(workspaces = true, windows = false, monitors = false),
            compositorFetchPlan(parsed),
        )
    }

    @Test
    fun compositorNavigationOnlyIncludesAvailableSections() {
        assertEquals(
            listOf(Destination.Home, Destination.System, Destination.Automation, Destination.Settings),
            visibleDestinations(CompositorCapabilities()),
        )
        assertEquals(
            listOf(Destination.Home, Destination.Workspaces, Destination.System, Destination.Automation, Destination.Settings),
            visibleDestinations(CompositorCapabilities(workspaces = true)),
        )
    }

    @Test
    fun unsupportedCompositorEndpointsAreExcludedFromRefreshPlan() {
        assertEquals(
            CompositorFetchPlan(workspaces = false, windows = false, monitors = false),
            compositorFetchPlan(CompositorCapabilities()),
        )
        assertEquals(
            CompositorFetchPlan(workspaces = true, windows = false, monitors = true),
            compositorFetchPlan(CompositorCapabilities(workspaces = true, monitors = true)),
        )
    }
}
