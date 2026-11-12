package dev.waycontrol

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SystemControlsRefreshTest {
    @Test
    fun controlStateRefreshRunsOnlyOnScreensThatDisplayIt() {
        assertTrue(shouldRefreshSystemControls(Destination.Home))
        assertTrue(shouldRefreshSystemControls(Destination.System))
        assertFalse(shouldRefreshSystemControls(Destination.Workspaces))
        assertFalse(shouldRefreshSystemControls(Destination.Windows))
        assertFalse(shouldRefreshSystemControls(Destination.Automation))
        assertFalse(shouldRefreshSystemControls(Destination.Settings))
    }
}
