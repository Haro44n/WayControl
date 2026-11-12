package dev.waycontrol

internal const val SYSTEM_CONTROLS_REFRESH_INTERVAL_MILLIS = 5_000L

internal fun shouldRefreshSystemControls(destination: Destination): Boolean =
    destination == Destination.Home || destination == Destination.System
