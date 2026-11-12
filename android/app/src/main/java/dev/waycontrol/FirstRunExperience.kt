package dev.waycontrol

internal fun shouldShowFirstRunWelcome(
    hasSavedConnection: Boolean,
    welcomeCompleted: Boolean,
): Boolean = !hasSavedConnection && !welcomeCompleted
