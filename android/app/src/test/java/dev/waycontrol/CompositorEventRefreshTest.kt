package dev.waycontrol

import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class CompositorEventRefreshTest {
    @Test
    fun eventBurstCoalescesIntoOneRefreshAndKeepsPeriodicPolling() = runBlocking {
        val signals = CompositorEventRefreshSignals(debounceMillis = 10)
        signals.signal()
        signals.signal()
        signals.signal()

        assertEquals(RefreshWakeReason.EVENT, signals.awaitRefresh(500, connected = true))
        assertEquals(RefreshWakeReason.PERIODIC, signals.awaitRefresh(1, connected = true))
    }

    @Test
    fun retryBackoffWaitIsNotInterruptedByQueuedEvents() = runBlocking {
        val signals = CompositorEventRefreshSignals(debounceMillis = 10)
        signals.signal()

        val startedAt = System.nanoTime()
        assertEquals(RefreshWakeReason.RETRY, signals.awaitRefresh(25, connected = false))
        val elapsedMillis = (System.nanoTime() - startedAt) / 1_000_000L
        assertTrue("queued events must not bypass reconnect delay", elapsedMillis >= 10)
        assertEquals(RefreshWakeReason.PERIODIC, signals.awaitRefresh(1, connected = true))
    }

    @Test
    fun eventArrivingDuringWaitTriggersDebouncedRefresh() = runBlocking {
        val signals = CompositorEventRefreshSignals(debounceMillis = 10)
        val sender = launch {
            delay(10)
            signals.signal()
        }

        assertEquals(RefreshWakeReason.EVENT, signals.awaitRefresh(500, connected = true))
        sender.join()
    }
}
