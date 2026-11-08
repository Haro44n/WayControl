package dev.waycontrol

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class HomeQuickWorkspaceSwitchTest {
    @get:Rule
    val composeRule = createComposeRule()

    private fun workspaces() = JSONArray()
        .put(JSONObject().put("id", "ws-main").put("name", "Main").put("active", true))
        .put(JSONObject().put("id", "ws-code").put("name", "Code").put("active", false))

    @Test
    fun selectingWorkspaceOnHomeDispatchesItsOpaqueId() {
        var selectedId: String? = null
        composeRule.setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    HomeScreen("Connected", false, null, JSONObject(), workspaces(), JSONArray(), 12, true) {
                        selectedId = it
                    }
                }
            }
        }

        composeRule.onNodeWithText("Code").performScrollTo().assertIsDisplayed().performClick()
        composeRule.runOnIdle { assertEquals("ws-code", selectedId) }
    }

    @Test
    fun workspaceSwitchButtonsStayDisabledWithoutCapability() {
        composeRule.setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    HomeScreen("Connected", false, null, JSONObject(), workspaces(), JSONArray(), null, false) { }
                }
            }
        }

        composeRule.onNodeWithText("Code").performScrollTo().assertIsNotEnabled()
    }
}
