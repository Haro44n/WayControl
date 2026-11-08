package dev.waycontrol

import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertNull
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.atomic.AtomicReference

@RunWith(AndroidJUnit4::class)
class DangerousActionConfirmationTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun shutdownRequiresConfirmationAndCancelKeepsItFromBeingDispatched() {
        val dispatchedAction = AtomicReference<String?>(null)
        composeRule.setContent {
            MaterialTheme {
                SystemScreen(JSONObject(), JSONArray(), "Connected") { operation, _ ->
                    dispatchedAction.set(operation)
                }
            }
        }

        composeRule.onNodeWithText("Shut down").performClick()

        composeRule.onNodeWithText("Shut down this computer?").assertIsDisplayed()
        assertNull(dispatchedAction.get())

        composeRule.onNodeWithText("Cancel").performClick()

        composeRule.onNodeWithText("Shut down this computer?").assertDoesNotExist()
        assertNull(dispatchedAction.get())
    }
}
