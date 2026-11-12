package dev.waycontrol

import org.junit.Assert.assertEquals
import org.junit.Test

class ReconnectBackoffTest {
    @Test
    fun failedReconnectsUseExponentialDelaysUpToThirtySeconds() {
        val backoff = ReconnectBackoff()

        assertEquals(1_000L, backoff.nextDelayMillis())
        assertEquals(2_000L, backoff.nextDelayMillis())
        assertEquals(4_000L, backoff.nextDelayMillis())
        assertEquals(8_000L, backoff.nextDelayMillis())
        assertEquals(16_000L, backoff.nextDelayMillis())
        assertEquals(30_000L, backoff.nextDelayMillis())
        assertEquals(30_000L, backoff.nextDelayMillis())
    }

    @Test
    fun successfulConnectionResetsTheNextRetryToOneSecond() {
        val backoff = ReconnectBackoff()
        backoff.nextDelayMillis()
        backoff.nextDelayMillis()

        backoff.reset()

        assertEquals(1_000L, backoff.nextDelayMillis())
    }

    @Test
    fun shortLivedEventConnectionsDoNotResetRetryDelay() {
        val backoff = ReconnectBackoff()
        backoff.nextDelayMillis()
        backoff.nextDelayMillis()

        backoff.resetAfterStableConnection(29_999)

        assertEquals(4_000L, backoff.nextDelayMillis())
    }

    @Test
    fun stableEventConnectionResetsRetryDelay() {
        val backoff = ReconnectBackoff()
        backoff.nextDelayMillis()
        backoff.nextDelayMillis()

        backoff.resetAfterStableConnection(30_000)

        assertEquals(1_000L, backoff.nextDelayMillis())
    }
}
