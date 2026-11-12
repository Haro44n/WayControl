package dev.waycontrol

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class FirstRunExperienceTest {
    @Test
    fun freshInstallWithoutSavedConnectionShowsWelcome() {
        assertTrue(shouldShowFirstRunWelcome(hasSavedConnection = false, welcomeCompleted = false))
    }

    @Test
    fun completedWelcomeStaysDismissedWithoutSavedConnection() {
        assertFalse(shouldShowFirstRunWelcome(hasSavedConnection = false, welcomeCompleted = true))
    }

    @Test
    fun restoredConnectionSkipsWelcome() {
        assertFalse(shouldShowFirstRunWelcome(hasSavedConnection = true, welcomeCompleted = false))
    }
}
