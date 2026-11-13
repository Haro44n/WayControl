package dev.waycontrol

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.assertHasNoClickAction
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
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

    @Test
    fun workspaceScreenAcceptsAnIdAndDispatchesSwitch() {
        var selectedId: String? = null
        composeRule.setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    WorkspaceScreen(JSONArray(), "Connected", true) { selectedId = it }
                }
            }
        }

        composeRule.onNodeWithText("Workspace ID").performTextInput(" dev-workspace ")
        composeRule.onNodeWithText("Go to workspace").performClick()
        composeRule.runOnIdle { assertEquals("dev-workspace", selectedId) }
    }

    @Test
    fun workspaceNavigationEntryIsHiddenWithoutSwitchCapability() {
        composeRule.setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    WorkspaceScreen(JSONArray(), "Read only", false) { }
                }
            }
        }

        composeRule.onAllNodesWithText("Go to workspace").assertCountEquals(0)
        composeRule.onAllNodesWithText("Workspace ID").assertCountEquals(0)
    }

    @Test
    fun workspaceSwitchControlsStayUnavailableAfterDisconnect() {
        composeRule.setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    WorkspaceScreen(workspaces(), "Disconnected", true) { }
                }
            }
        }

        composeRule.onNodeWithText("Connect to switch workspaces").performScrollTo().assertIsDisplayed()
        composeRule.onAllNodesWithText("Workspace ID").assertCountEquals(0)
        composeRule.onAllNodesWithText("Go to workspace").assertCountEquals(0)
        composeRule.onNodeWithText("Code").performScrollTo().assertHasNoClickAction()
    }

    @Test
    fun windowActionsStayUnavailableAfterDisconnect() {
        val window = JSONObject()
            .put("address", "0x1")
            .put("title", "Editor")
            .put("app_id", "editor")
            .put("workspace_id", "ws-code")
        val capabilities = CompositorCapabilities(focusWindow = true, closeWindow = true)
        composeRule.setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    WindowScreen(JSONArray().put(window), workspaces(), JSONArray(), "Disconnected", capabilities) { _, _, _ -> }
                }
            }
        }

        composeRule.onNodeWithText("Connect to manage windows").assertIsDisplayed()
        composeRule.onAllNodesWithText("Focus").assertCountEquals(0)
        composeRule.onAllNodesWithText("Close").assertCountEquals(0)
    }

    @Test
    fun homePowerActionRequiresConfirmationAndCancelDoesNotDispatch() {
        val calls = mutableListOf<Pair<String, JSONObject>>()
        composeRule.setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    HomeScreen(
                        "Connected", false, null, JSONObject(), workspaces(), JSONArray(), null, true,
                        action = { operation, payload -> calls += operation to payload },
                    ) { }
                }
            }
        }

        composeRule.onNodeWithText("Suspend").performClick()
        composeRule.onNodeWithText("Suspend this computer?").assertIsDisplayed()
        composeRule.onNodeWithText("Cancel").performClick()
        composeRule.runOnIdle { assertEquals(0, calls.size) }

        composeRule.onNodeWithText("Suspend").performClick()
        composeRule.onNodeWithText("Confirm Suspend").performClick()
        composeRule.runOnIdle {
            assertEquals(1, calls.size)
            assertEquals("system.power", calls.single().first)
            assertEquals("suspend", calls.single().second.optString("action"))
        }
    }

    @Test
    fun quickActionsAreDisabledWhileDisconnectedEvenWhenBackendDataIsPresent() {
        val capabilities = CompositorCapabilities(lockScreen = true, monitors = true, monitorPower = true)
        composeRule.setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    HomeScreen(
                        "Disconnected", false, null, JSONObject(), workspaces(), JSONArray(), null, false,
                        capabilities = capabilities,
                        monitors = JSONArray().put(JSONObject().put("name", "DP-1").put("dpms_enabled", true)),
                        audio = JSONObject().put("available", true),
                        media = JSONObject().put("available", true),
                    ) { }
                }
            }
        }

        composeRule.onNodeWithText("Lock").performScrollTo().assertIsNotEnabled()
        composeRule.onNodeWithText("Turn off DP-1").performScrollTo().assertIsNotEnabled()
        composeRule.onNodeWithText("Volume +").performScrollTo().assertIsNotEnabled()
        composeRule.onNodeWithText("Play / pause").performScrollTo().assertIsNotEnabled()
        composeRule.onNodeWithText("Suspend").performScrollTo().assertIsNotEnabled()
        composeRule.onNodeWithText("Reboot").performScrollTo().assertIsNotEnabled()
        composeRule.onNodeWithText("Shut down").performScrollTo().assertIsNotEnabled()
    }

    @Test
    fun homeAudioShortcutsStayHiddenWhenBackendIsUnavailable() {
        composeRule.setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    HomeScreen("Connected", false, null, JSONObject(), workspaces(), JSONArray(), null, true) { }
                }
            }
        }
        composeRule.onNodeWithText("Audio controls unavailable").assertIsDisplayed()
        composeRule.onAllNodesWithText("Volume +").assertCountEquals(0)
        composeRule.onNodeWithText("Media controls unavailable").assertIsDisplayed()
    }

    @Test
    fun connectionStatusRemainsHorizontalBesideLongBackendError() {
        composeRule.setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    HomeScreen(
                        status = "Connected",
                        loading = false,
                        error = "Hyprland instance is unavailable: cannot enumerate Hyprland instances in /tmp/runtime/hypr: No such file or directory",
                        desktop = JSONObject(),
                        workspaces = JSONArray(),
                        windows = JSONArray(),
                        networkLatencyMs = null,
                        canSwitchWorkspace = false,
                        switchWorkspace = {},
                    )
                }
            }
        }

        val status = composeRule.onNodeWithText("CONNECTED")
        val bounds = status.getUnclippedBoundsInRoot()
        val width = bounds.right - bounds.left
        val height = bounds.bottom - bounds.top
        assertTrue("Connection status should stay on one line: $bounds", width > height)
    }

    @Test
    fun homeMicrophoneMuteShortcutDispatchesTypedAction() {
        val actions = mutableListOf<Pair<String, JSONObject>>()
        composeRule.setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    HomeScreen(
                        "Connected", false, null, JSONObject(), workspaces(), JSONArray(), null, true,
                        audio = JSONObject()
                            .put("available", true)
                            .put("output_muted", false)
                            .put("microphone_muted", true),
                        action = { operation, payload -> actions += operation to payload },
                    ) { }
                }
            }
        }
        composeRule.onNodeWithText("Unmute mic").performClick()
        composeRule.runOnIdle {
            assertEquals("audio.microphone_mute", actions.single().first)
        }
    }
}
