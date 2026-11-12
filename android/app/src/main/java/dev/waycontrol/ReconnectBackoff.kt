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
}
