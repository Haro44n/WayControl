package dev.waycontrol

import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.withTimeoutOrNull

internal enum class RefreshWakeReason {
    EVENT,
    PERIODIC,
    RETRY,
}

/** Coalesces compositor event bursts without bypassing reconnect backoff. */
internal class CompositorEventRefreshSignals(
    private val debounceMillis: Long = DEFAULT_DEBOUNCE_MILLIS,
) {
    private val signals = Channel<Unit>(Channel.CONFLATED)

    init {
        require(debounceMillis >= 0)
    }

    fun signal() {
        signals.trySend(Unit)
    }

    suspend fun awaitRefresh(delayMillis: Long, connected: Boolean): RefreshWakeReason {
        require(delayMillis > 0)
        if (!connected) {
            delay(delayMillis)
            discardPendingSignals()
            return RefreshWakeReason.RETRY
        }

        val received = withTimeoutOrNull(delayMillis) {
            signals.receive()
            true
        } ?: false
        if (!received) return RefreshWakeReason.PERIODIC

        delay(debounceMillis)
        discardPendingSignals()
        return RefreshWakeReason.EVENT
    }

    private fun discardPendingSignals() {
        while (signals.tryReceive().isSuccess) Unit
    }

    private companion object {
        const val DEFAULT_DEBOUNCE_MILLIS = 150L
    }
}
