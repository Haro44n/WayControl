package dev.waycontrol

import android.content.Context
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class FirstRunExperienceUiTest {
    @get:Rule
    val composeRule = createComposeRule()

    private val context get() = InstrumentationRegistry.getInstrumentation().targetContext

    private fun clearFirstRunState() {
        context.getSharedPreferences("trusted_connection", Context.MODE_PRIVATE).edit().clear().commit()
        context.getSharedPreferences("first_run", Context.MODE_PRIVATE).edit().clear().commit()
    }

    @Test
    fun freshInstallDiscoversAndCanContinueToHomeWithPairingInSettings() {
        clearFirstRunState()

        composeRule.setContent {
            WayControlApp(SecureConnectionStore(context))
        }

        composeRule.onNodeWithText("Welcome to WayControl").assertIsDisplayed()
        composeRule.onNodeWithText("Find a desktop").assertIsDisplayed()
        composeRule.onNodeWithText("Server address").assertIsDisplayed()
        composeRule.onNodeWithText("Scan pairing QR").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("Certificate SHA-256").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("Pairing code").performScrollTo().assertIsDisplayed()
        composeRule.waitUntil(5_000) {
            composeRule.onAllNodesWithText("No WayControl servers found on this network")
                .fetchSemanticsNodes().isNotEmpty()
        }

        composeRule.onNodeWithText("Continue without pairing").performScrollTo().performClick()
        composeRule.onNodeWithText("Welcome to WayControl").assertDoesNotExist()
        composeRule.onNodeWithText("Desktop").assertExists()
        composeRule.onNodeWithText("Desktop").assertIsDisplayed()
        assertTrue(context.getSharedPreferences("first_run", Context.MODE_PRIVATE)
            .getBoolean("welcome_completed", false))

        composeRule.onNodeWithText("Settings").performClick()
        composeRule.onNodeWithText("Pair this device").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("Find on local network").performScrollTo().assertIsDisplayed()
    }

    @Test
    fun completedWelcomeOpensDashboardWithoutShowingWelcome() {
        clearFirstRunState()
        context.getSharedPreferences("first_run", Context.MODE_PRIVATE).edit()
            .putBoolean("welcome_completed", true).commit()

        composeRule.setContent {
            WayControlApp(SecureConnectionStore(context))
        }

        composeRule.onNodeWithText("Welcome to WayControl").assertDoesNotExist()
        composeRule.onNodeWithText("Desktop").assertIsDisplayed()
    }
}
