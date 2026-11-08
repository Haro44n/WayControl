package dev.waycontrol

import org.junit.Assert.assertEquals
import org.junit.Test

class TelemetryRefreshSettingsTest {
    @Test
    fun defaultKeepsFastScreensAtOneSecondAndOtherScreensAtFiveSeconds() {
        val interval = TelemetryRefreshInterval.default

        assertEquals(1_000L, interval.delayMillis(Destination.Home))
        assertEquals(1_000L, interval.delayMillis(Destination.System))
        assertEquals(5_000L, interval.delayMillis(Destination.Workspaces))
        assertEquals(5_000L, interval.delayMillis(Destination.Windows))
        assertEquals(5_000L, interval.delayMillis(Destination.Settings))
    }

    @Test
    fun slowerIntervalsScaleWithScreenPriorityAndPersistAsSeconds() {
        val interval = TelemetryRefreshInterval.fromSeconds(5)

        assertEquals(5_000L, interval.delayMillis(Destination.Home))
        assertEquals(5_000L, interval.delayMillis(Destination.System))
        assertEquals(25_000L, interval.delayMillis(Destination.Workspaces))
        assertEquals(5, interval.seconds)
        assertEquals(TelemetryRefreshInterval.TWO_SECONDS, TelemetryRefreshInterval.fromSeconds(2))
    }

    @Test
    fun unsupportedStoredIntervalsFallBackToTheDefault() {
        assertEquals(TelemetryRefreshInterval.default, TelemetryRefreshInterval.fromSeconds(3))
        assertEquals(TelemetryRefreshInterval.default, TelemetryRefreshInterval.fromSeconds(0))
    }
}
