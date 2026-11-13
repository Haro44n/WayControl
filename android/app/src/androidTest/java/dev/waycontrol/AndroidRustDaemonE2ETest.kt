package dev.waycontrol

import android.content.Context
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class AndroidRustDaemonE2ETest {
    @get:Rule
    val composeRule = createComposeRule()

    private val context get() = InstrumentationRegistry.getInstrumentation().targetContext

    @Test
    fun freshAndroidInstallPairsWithRustDaemonAndDisplaysLiveDesktop() {
        val arguments = InstrumentationRegistry.getArguments()
        val endpoint = arguments.getString("waycontrolEndpoint")
        val fingerprint = arguments.getString("waycontrolFingerprint")
        val pairingCode = arguments.getString("waycontrolPairingCode")
        val expectedComputerName = arguments.getString("waycontrolComputerName")
        assumeTrue(
            "The Rust daemon pairing harness supplies endpoint, fingerprint, code, and hostname",
            listOf(endpoint, fingerprint, pairingCode, expectedComputerName).all { !it.isNullOrBlank() },
        )

        context.getSharedPreferences("trusted_connection", Context.MODE_PRIVATE).edit().clear().commit()
        context.getSharedPreferences("first_run", Context.MODE_PRIVATE).edit().clear().commit()
        val store = SecureConnectionStore(context)

        composeRule.setContent { WayControlApp(store) }
        composeRule.onNodeWithText("Server address").performScrollTo().performTextInput(endpoint!!)
        composeRule.onNodeWithText("Certificate SHA-256").performScrollTo().performTextInput(fingerprint!!)
        composeRule.onNodeWithText("Pairing code").performScrollTo().performTextInput(pairingCode!!)
        composeRule.onNodeWithText("Pair this device").performScrollTo().performClick()

        composeRule.waitUntil(90_000) {
            composeRule.onAllNodesWithText("CONNECTED").fetchSemanticsNodes().isNotEmpty()
        }
        composeRule.onNodeWithText("CONNECTED").assertExists()
        composeRule.waitUntil(30_000) {
            composeRule.onAllNodesWithText(expectedComputerName!!).fetchSemanticsNodes().isNotEmpty()
        }
        composeRule.onNodeWithText(expectedComputerName!!).assertExists()

        val saved = assertNotNullAndReturn(store.load())
        assertEquals(endpoint, saved.first)
        assertEquals(fingerprint.lowercase(), saved.second)
        assertTrue("The Android Keystore-backed credential must be restored", saved.third.isNotBlank())
    }

    private fun assertNotNullAndReturn(value: Triple<String, String, String>?): Triple<String, String, String> {
        assertNotNull("The app must persist the approved device credential", value)
        return requireNotNull(value)
    }
}
