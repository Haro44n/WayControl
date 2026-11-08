package dev.waycontrol

import androidx.compose.material3.MaterialTheme
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.rememberScrollState
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.foundation.verticalScroll
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.performSemanticsAction
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.atomic.AtomicReference
import java.util.concurrent.CopyOnWriteArrayList

@RunWith(AndroidJUnit4::class)
class CompositorIndependentSystemControlsTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun perCoreCpuTemperaturesRenderWithoutCompositorCapabilities() {
        val desktop = JSONObject().put(
            "cpu_core_temperatures",
            JSONArray().put(
                JSONObject()
                    .put("package_id", 0)
                    .put("core_id", 4)
                    .put("temperature_millidegrees", 37_250),
            ),
        )
        composeRule.setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    SystemScreen(desktop, JSONArray(), "Connected", action = { _, _ -> })
                }
            }
        }

        composeRule.onNodeWithText("CPU core temperatures").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("Package 0 · Core 4").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("37.3 °C").performScrollTo().assertIsDisplayed()
    }

    @Test
    fun storageCapacityRendersWithoutCompositorCapabilities() {
        val desktop = JSONObject().put(
            "storage",
            JSONArray().put(
                JSONObject()
                    .put("mount_point", "/home")
                    .put("source", "/dev/nvme0n1p2")
                    .put("filesystem_type", "btrfs")
                    .put("total_bytes", 1_000_000_000)
                    .put("used_bytes", 725_000_000)
                    .put("available_bytes", 250_000_000),
            ),
        )
        composeRule.setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    SystemScreen(desktop, JSONArray(), "Connected", action = { _, _ -> })
                }
            }
        }

        composeRule.onNodeWithText("Storage").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("/home").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("/dev/nvme0n1p2 · btrfs").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("72.5% used · 691.9 MiB / 953.7 MiB · 238.4 MiB free")
            .performScrollTo().assertIsDisplayed()
    }

    @Test
    fun networkInterfaceAddressesAndRatesRenderWithoutCompositorCapabilities() {
        val desktop = JSONObject()
            .put(
                "network_interfaces",
                JSONArray().put(
                    JSONObject()
                        .put("name", "wlan0")
                        .put("is_up", true)
                        .put("operational_state", "up")
                        .put("default_route", true)
                        .put("addresses", JSONArray().put("192.168.1.10").put("fe80::1")),
                ),
            )
            .put(
                "network_rates",
                JSONArray().put(
                    JSONObject()
                        .put("name", "wlan0")
                        .put("received_bytes_per_second", 1024.0)
                        .put("transmitted_bytes_per_second", 2048.0),
                ),
            )
        composeRule.setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    SystemScreen(desktop, JSONArray(), "Connected", action = { _, _ -> })
                }
            }
        }

        composeRule.onNodeWithText("Network interfaces").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("Up · Default route").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("192.168.1.10 · fe80::1").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("↓ 1.0 KiB/s · ↑ 2.0 KiB/s").performScrollTo().assertIsDisplayed()
    }

    @Test
    fun optionalGpuFanClockAndPowerLimitRenderWhenAvailable() {
        val desktop = JSONObject().put(
            "gpu",
            JSONObject()
                .put("available", true)
                .put("name", "Test GPU")
                .put("fan_speed_percent", 37)
                .put("graphics_clock_mhz", 1815)
                .put("power_limit_watts", 250.0),
        )
        composeRule.setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    SystemScreen(desktop, JSONArray(), "Connected", action = { _, _ -> })
                }
            }
        }

        composeRule.onNodeWithText("GPU fan").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("37%").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("1815 MHz").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("250.0 W").performScrollTo().assertIsDisplayed()
    }

    @Test
    fun pairingQrActionIsAvailableAndDispatchesScanRequest() {
        val scanRequested = AtomicReference(false)
        composeRule.setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    PairingQrSection(scanning = false, onScan = { scanRequested.set(true) })
                }
            }
        }

        composeRule.onNodeWithText("Scan pairing QR").assertIsDisplayed().performClick()
        assertEquals(true, scanRequested.get())
    }

    @Test
    fun telemetryRefreshSelectorDispatchesTheSelectedInterval() {
        val selectedInterval = AtomicReference<TelemetryRefreshInterval?>(null)
        composeRule.setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    TelemetryRefreshSection(TelemetryRefreshInterval.default) { selectedInterval.set(it) }
                }
            }
        }

        composeRule.onNodeWithText("1 second").performClick()
        composeRule.onNodeWithText("2 seconds").performClick()

        assertEquals(TelemetryRefreshInterval.TWO_SECONDS, selectedInterval.get())
    }

    @Test
    fun systemScreenShowsLoadAverageAndPerCoreCpuValues() {
        val desktop = JSONObject()
            .put("load_average", JSONObject().put("one_minute", 0.42).put("five_minutes", 0.31).put("fifteen_minutes", 0.20))
            .put("cpu_cores", JSONArray().put(JSONObject().put("id", "cpu0").put("usage_percent", 62.5).put("frequency_khz", 2_400_000)))
        composeRule.setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    SystemScreen(desktop, JSONArray(), "Connected", CompositorCapabilities()) { _, _ -> }
                }
            }
        }

        composeRule.onNodeWithText("Load average").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("0.42 · 0.31 · 0.20").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("CPU cores").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("cpu0").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("62.5% · 2400 MHz").performScrollTo().assertIsDisplayed()
    }

    @Test
    fun lanDiscoveryListsServersAndSelectingOneOnlyFillsItsAddress() {
        val selectedEndpoint = AtomicReference<String?>(null)
        val scanRequested = AtomicReference(false)
        composeRule.setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    LanDiscoverySection(
                        listOf(DiscoveredServer("Living room", "https://192.168.1.20:8765")),
                        "Found 1 server(s)",
                        false,
                        { scanRequested.set(true) },
                        { selectedEndpoint.set(it) },
                    )
                }
            }
        }

        composeRule.onNodeWithText("Living room").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("Find on local network").performClick()
        composeRule.onNodeWithText("Use address").performClick()

        assertEquals(true, scanRequested.get())
        assertEquals("https://192.168.1.20:8765", selectedEndpoint.get())
    }

    @Test
    fun audioAndMediaControlsRemainAvailableWithoutMonitorCapability() {
        val dispatchedAction = AtomicReference<String?>(null)
        val dispatchedDirection = AtomicReference<String?>(null)
        composeRule.setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    val monitorData = JSONArray().put(JSONObject().put("name", "Test display").put("dpms_enabled", true))
                    SystemScreen(JSONObject(), monitorData, "Connected", CompositorCapabilities()) { operation, payload ->
                        dispatchedAction.set(operation)
                        dispatchedDirection.set(payload.optString("direction"))
                    }
                }
            }
        }

        composeRule.onNodeWithText("Volume +").performScrollTo().assertIsDisplayed().performClick()
        composeRule.onNodeWithText("Toggle mic mute").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("Play / pause").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("Suspend").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("Shut down").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("Reboot").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("Test display").performScrollTo().assertIsDisplayed()
        composeRule.onAllNodesWithText("Lock screen").assertCountEquals(0)
        composeRule.onAllNodesWithText("Turn off").assertCountEquals(0)

        assertEquals("audio.volume", dispatchedAction.get())
        assertEquals("up", dispatchedDirection.get())
    }

    @Test
    fun outputSliderShowsCurrentVolumeAndDispatchesTheSelectedLevel() {
        val dispatchedAction = AtomicReference<String?>(null)
        val dispatchedPercent = AtomicReference<Int?>(null)
        val audio = JSONObject().put("available", true).put("output_volume_percent", 42).put("output_muted", false)
        composeRule.setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    SystemScreen(JSONObject(), JSONArray(), "Connected", CompositorCapabilities(), audio) { operation, payload ->
                        dispatchedAction.set(operation)
                        dispatchedPercent.set(payload.optInt("percent"))
                    }
                }
            }
        }

        composeRule.onNodeWithText("Output volume · 42%").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithContentDescription("Output volume").performSemanticsAction(SemanticsActions.SetProgress) { setProgress ->
            assert(setProgress(65f))
        }

        assertEquals("audio.volume.set", dispatchedAction.get())
        assertEquals(65, dispatchedPercent.get())
    }

    @Test
    fun microphoneButtonReflectsTheReportedMuteState() {
        val audio = JSONObject()
            .put("available", true)
            .put("output_volume_percent", 30)
            .put("microphone_muted", true)
        composeRule.setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    SystemScreen(JSONObject(), JSONArray(), "Connected", CompositorCapabilities(), audio) { _, _ -> }
                }
            }
        }

        composeRule.onNodeWithText("Microphone muted").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("Unmute microphone").performScrollTo().assertIsDisplayed()
    }

    @Test
    fun mprisPlayerMetadataAndTransportButtonsTargetDiscoveredPlayer() {
        val dispatched = AtomicReference<Pair<String, String>?>(null)
        val media = JSONObject().put("available", true).put("players", JSONArray().put(
            JSONObject()
                .put("player_id", "org.mpris.MediaPlayer2.spotify")
                .put("name", "spotify")
                .put("playback_status", "Playing")
                .put("title", "Night Drive")
                .put("artist", "Example Artist")
                .put("album", "After Dark"),
        ))
        composeRule.setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    SystemScreen(JSONObject(), JSONArray(), "Connected", CompositorCapabilities(), JSONObject(), media) { operation, payload ->
                        dispatched.set(operation to payload.optString("player_id"))
                    }
                }
            }
        }

        composeRule.onNodeWithText("Night Drive").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("Example Artist · After Dark").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("Next").performScrollTo().assertIsDisplayed().performClick()
        assertEquals("media.next" to "org.mpris.MediaPlayer2.spotify", dispatched.get())
    }

    @Test
    fun outputAndInputMenusDispatchOnlyTheirSelectedDeviceIds() {
        val dispatched = CopyOnWriteArrayList<Pair<String, Long>>()
        val audio = JSONObject()
            .put("available", true)
            .put("output_volume_percent", 30)
            .put("output_devices", JSONArray()
                .put(JSONObject().put("id", 53L).put("name", "Speaker").put("active", true))
                .put(JSONObject().put("id", 4_294_967_290L).put("name", "Headphones").put("active", false)))
            .put("input_devices", JSONArray()
                .put(JSONObject().put("id", 63L).put("name", "Webcam mic").put("active", true))
                .put(JSONObject().put("id", 4_294_967_291L).put("name", "Desk mic").put("active", false)))
        composeRule.setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    SystemScreen(JSONObject(), JSONArray(), "Connected", CompositorCapabilities(), audio) { operation, payload ->
                        dispatched += operation to payload.optLong("id")
                    }
                }
            }
        }

        composeRule.onNodeWithText("Output: Speaker").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("Input: Webcam mic").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("Output: Speaker").performClick()
        composeRule.onNodeWithText("Headphones").performClick()
        composeRule.onNodeWithText("Input: Webcam mic").performClick()
        composeRule.onNodeWithText("Desk mic").performClick()

        assertEquals(
            listOf("audio.output.select" to 4_294_967_290L, "audio.input.select" to 4_294_967_291L),
            dispatched.toList(),
        )
    }

    @Test
    fun outputMuteButtonShowsStateAndDispatchesTheToggle() {
        val dispatchedAction = AtomicReference<String?>(null)
        val audio = JSONObject()
            .put("available", true)
            .put("output_volume_percent", 40)
            .put("output_muted", false)
        composeRule.setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    SystemScreen(JSONObject(), JSONArray(), "Connected", CompositorCapabilities(), audio) { operation, _ ->
                        dispatchedAction.set(operation)
                    }
                }
            }
        }

        composeRule.onNodeWithText("Output active").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("Mute output").performScrollTo().assertIsDisplayed().performClick()
        assertEquals("audio.output_mute", dispatchedAction.get())
    }

    @Test
    fun mutedOutputShowsUnmuteAction() {
        val mutedAudio = JSONObject()
            .put("available", true)
            .put("output_volume_percent", 40)
            .put("output_muted", true)
        composeRule.setContent {
            MaterialTheme {
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    SystemScreen(JSONObject(), JSONArray(), "Connected", CompositorCapabilities(), mutedAudio) { _, _ -> }
                }
            }
        }
        composeRule.onNodeWithText("Output muted").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("Unmute output").performScrollTo().assertIsDisplayed()
    }
}
