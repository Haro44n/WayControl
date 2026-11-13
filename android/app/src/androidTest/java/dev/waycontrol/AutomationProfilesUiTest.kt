package dev.waycontrol

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.atomic.AtomicReference

@RunWith(AndroidJUnit4::class)
class AutomationProfilesUiTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun runButtonStaysDisabledWhenTheDesktopConnectionIsDown() {
        val profile = AutomationProfile(
            id = "away",
            name = "Away",
            icon = "🌙",
            actions = listOf(AutomationAction(AutomationActionType.PAUSE_MEDIA)),
        )
        composeRule.setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    AutomationScreen(
                        profiles = listOf(profile),
                        result = null,
                        connected = false,
                        onSave = {},
                        onDelete = {},
                        onRun = {},
                    )
                }
            }
        }

        composeRule.onNodeWithText("Run").performScrollTo().assertIsNotEnabled()
    }

    @Test
    fun settingsEntryOpensAutomationProfiles() {
        val opened = AtomicReference(false)
        composeRule.setContent {
            MaterialTheme {
                AutomationSettingsEntry { opened.set(true) }
            }
        }

        composeRule.onNodeWithText("Manage automation profiles").performClick()
        composeRule.runOnIdle { assertTrue(opened.get()) }
    }

    @Test
    fun profilesListShowsPresetsAndNewProfileSavesValidatedLaunchAction() {
        val savedProfile = AtomicReference<AutomationProfile?>(null)
        composeRule.setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    AutomationScreen(
                        profiles = defaultAutomationProfiles(),
                        result = null,
                        connected = true,
                        onSave = savedProfile::set,
                        onDelete = {},
                        onRun = {},
                    )
                }
            }
        }

        listOf("Gaming", "Development", "Away", "Eco", "Custom").forEach { name ->
            composeRule.onNodeWithText(name, substring = true).performScrollTo().assertIsDisplayed()
        }

        composeRule.onNodeWithText("Add profile").performScrollTo().performClick()
        composeRule.onNodeWithText("Desktop application ID").performTextInput("org.mozilla.Firefox")
        composeRule.onNodeWithText("Add action").performClick()
        composeRule.onNodeWithText("Save").performClick()

        composeRule.runOnIdle {
            val profile = savedProfile.get()
            assertNotNull(profile)
            assertEquals("My profile", profile?.name)
            assertEquals(AutomationActionType.LAUNCH_APPLICATION, profile?.actions?.single()?.type)
            assertEquals("org.mozilla.Firefox", profile?.actions?.single()?.value)
        }
    }
}
