package dev.waycontrol

internal enum class TelemetryRefreshInterval(val seconds: Int, val label: String) {
    ONE_SECOND(1, "1 second"),
    TWO_SECONDS(2, "2 seconds"),
    FIVE_SECONDS(5, "5 seconds");

    fun delayMillis(destination: Destination): Long {
        val multiplier = if (destination == Destination.Home || destination == Destination.System) 1 else 5
        return seconds * multiplier * 1_000L
    }

    companion object {
        val default = ONE_SECOND

        fun fromSeconds(seconds: Int): TelemetryRefreshInterval =
            entries.firstOrNull { it.seconds == seconds } ?: default
    }
}
