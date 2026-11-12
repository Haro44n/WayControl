package dev.waycontrol

internal class ReconnectBackoff(
    private val initialDelayMillis: Long = 1_000,
    private val maxDelayMillis: Long = 30_000,
) {
    private var nextDelayMillis = initialDelayMillis

    init {
        require(initialDelayMillis > 0)
        require(maxDelayMillis >= initialDelayMillis)
    }

    fun nextDelayMillis(): Long {
        val delay = nextDelayMillis
        nextDelayMillis = if (delay >= maxDelayMillis / 2) {
            maxDelayMillis
        } else {
            delay * 2
        }
        return delay
    }

    fun reset() {
        nextDelayMillis = initialDelayMillis
    }

    fun resetAfterStableConnection(durationMillis: Long) {
        require(durationMillis >= 0)
        if (durationMillis >= STABLE_CONNECTION_MILLIS) reset()
    }

    private companion object {
        const val STABLE_CONNECTION_MILLIS = 30_000L
    }
}
