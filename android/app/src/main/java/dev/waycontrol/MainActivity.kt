package dev.waycontrol

import android.os.Bundle
import android.graphics.Bitmap
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.clickable
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Checkbox
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.Alignment
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.google.mlkit.vision.codescanner.GmsBarcodeScannerOptions
import com.google.mlkit.vision.codescanner.GmsBarcodeScanning
import com.google.mlkit.vision.barcode.common.Barcode
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.isActive
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import org.json.JSONArray
import org.json.JSONObject
import java.util.Locale
import kotlin.math.roundToInt

private val Background = Color(0xFF0B0B0E)
private val SurfaceColor = Color(0xFF1C1C22)
private val Elevated = Color(0xFF25252C)
private val MainText = Color(0xFFF3F3F5)
private val SecondaryText = Color(0xFF9999A4)
private val Accent = Color(0xFF8C3655)

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val store = SecureConnectionStore(this)
        setContent { WayControlApp(store) }
    }
}

internal enum class Destination(val label: String, val glyph: String) {
    Home("Home", "⌂"), Workspaces("Workspaces", "▦"), Windows("Windows", "▣"),
    System("System", "◷"), Automation("Profiles", "⚡"), Settings("Settings", "⚙")
}

internal fun visibleDestinations(capabilities: CompositorCapabilities): List<Destination> = buildList {
    add(Destination.Home)
    if (capabilities.workspaces) add(Destination.Workspaces)
    if (capabilities.windows) add(Destination.Windows)
    add(Destination.System)
    add(Destination.Automation)
    add(Destination.Settings)
}

@Composable
internal fun WayControlApp(store: SecureConnectionStore) {
    val context = LocalContext.current
    val lifecycle = (context as? LifecycleOwner)?.lifecycle
    val telemetryPreferences = remember(context) {
        context.getSharedPreferences("telemetry_settings", android.content.Context.MODE_PRIVATE)
    }
    val firstRunPreferences = remember(context) {
        context.getSharedPreferences("first_run", android.content.Context.MODE_PRIVATE)
    }
    var refreshInterval by remember(context) {
        mutableStateOf(
            TelemetryRefreshInterval.fromSeconds(
                telemetryPreferences.getInt("refresh_interval_seconds", TelemetryRefreshInterval.default.seconds),
            ),
        )
    }
    var selected by rememberSaveable { mutableStateOf(Destination.Home) }
    var showWelcome by rememberSaveable { mutableStateOf<Boolean?>(null) }
    var endpoint by rememberSaveable { mutableStateOf("") }
    var pin by rememberSaveable { mutableStateOf("") }
    var code by rememberSaveable { mutableStateOf("") }
    var token by remember { mutableStateOf<String?>(null) }
    var status by remember { mutableStateOf("Disconnected") }
    var error by remember { mutableStateOf<String?>(null) }
    var loading by remember { mutableStateOf(false) }
    var snapshot by remember { mutableStateOf(RemoteSnapshot()) }
    var telemetryHistory by remember { mutableStateOf(TelemetryHistory()) }
    var mediaArtwork by remember { mutableStateOf<MediaArtworkImage?>(null) }
    val automationStore = remember(context) { AutomationProfileStore(context) }
    var automationProfiles by remember(context) { mutableStateOf(automationStore.load()) }
    var automationResult by remember { mutableStateOf<JSONObject?>(null) }
    var profileConfirmation by remember { mutableStateOf<AutomationProfile?>(null) }
    val compositorEventRefreshSignals = remember { CompositorEventRefreshSignals() }
    var discoveredServers by remember { mutableStateOf(emptyList<DiscoveredServer>()) }
    var discoveryStatus by remember { mutableStateOf<String?>(null) }
    var discovering by remember { mutableStateOf(false) }
    var scanningQr by remember { mutableStateOf(false) }
    val contentScrollState = rememberScrollState()
    val scope = rememberCoroutineScope()
    val capabilities = remember(snapshot.desktop) { CompositorCapabilities.fromDesktop(snapshot.desktop) }
    val destinations = remember(capabilities) { visibleDestinations(capabilities) }
    val artworkTarget = remember(snapshot.media) { mediaArtworkTarget(snapshot.media) }
    val currentDestination by rememberUpdatedState(selected)

    fun markWelcomeCompleted() {
        firstRunPreferences.edit().putBoolean("welcome_completed", true).apply()
        showWelcome = false
    }

    fun startDiscovery() {
        if (discovering) return
        discovering = true
        discoveryStatus = null
        scope.launch {
            try {
                discoveredServers = LanDiscovery.discover()
                discoveryStatus = if (discoveredServers.isEmpty()) {
                    "No WayControl servers found on this network"
                } else {
                    "Found ${discoveredServers.size} server(s)"
                }
            } catch (exception: Exception) {
                discoveryStatus = exception.message ?: "Network discovery failed"
            } finally {
                discovering = false
            }
        }
    }

    fun runAutomation(profile: AutomationProfile, confirmed: Boolean) {
        scope.launch {
            automationResult = withContext(Dispatchers.IO) {
                runCatching {
                    val savedToken = token ?: error("Connect to a desktop before running a profile")
                    executeAutomationProfile(profile, confirmed) { operation, payload ->
                        WayControlClient(endpoint, pin, savedToken).request(operation, payload)
                    }
                }.getOrElse { failure ->
                    JSONObject()
                        .put("profile_id", profile.id)
                        .put("succeeded", false)
                        .put("summary", failure.message ?: "Profile execution failed")
                        .put("steps", JSONArray())
                }
            }
        }
    }

    LaunchedEffect(destinations, selected) {
        if (selected !in destinations) selected = Destination.Home
    }

    LaunchedEffect(Unit) {
        val savedConnection = store.load()
        savedConnection?.let { (savedEndpoint, savedPin, savedToken) ->
            endpoint = savedEndpoint; pin = savedPin; token = savedToken; status = "Connecting"
        }
        val welcomeCompleted = firstRunPreferences.getBoolean("welcome_completed", false)
        showWelcome = shouldShowFirstRunWelcome(
            hasSavedConnection = savedConnection != null,
            welcomeCompleted = welcomeCompleted,
        )
        if (savedConnection != null && !welcomeCompleted) markWelcomeCompleted()
        if (showWelcome == true) {
            selected = Destination.Settings
            startDiscovery()
        }
    }
    LaunchedEffect(showWelcome) {
        if (showWelcome == false) contentScrollState.scrollTo(0)
    }
    LaunchedEffect(token, endpoint, pin, selected, refreshInterval) {
        if (token == null || endpoint.isBlank() || pin.isBlank()) return@LaunchedEffect
        suspend fun refresh(): Boolean {
            loading = true
            val connected = try {
                val updated = withContext(Dispatchers.IO) {
                    val client = WayControlClient(endpoint, pin, token)
                    val desktopStartedAt = System.nanoTime()
                    val desktop = client.request("desktop.info")
                    val networkLatencyMs = ((System.nanoTime() - desktopStartedAt) / 1_000_000L).coerceAtLeast(0)
                    val fetch = compositorFetchPlan(CompositorCapabilities.fromDesktop(desktop))
                    RemoteSnapshot(
                        desktop = desktop,
                        workspaces = if (fetch.workspaces) client.request("workspace.list").optJSONArray("items") ?: JSONArray() else JSONArray(),
                        windows = if (fetch.windows) client.request("window.list").optJSONArray("items") ?: JSONArray() else JSONArray(),
                        monitors = if (fetch.monitors) client.request("monitor.list").optJSONArray("items") ?: JSONArray() else JSONArray(),
                        networkLatencyMs = networkLatencyMs,
                    )
                }
                if (selected == Destination.System) {
                    telemetryHistory = telemetryHistory.append(telemetrySample(updated.desktop))
                }
                snapshot = updated.copy(audio = snapshot.audio, media = snapshot.media)
                status = "Connected"; error = null
                true
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                snapshot = snapshot.cleared()
                telemetryHistory = TelemetryHistory()
                status = "Disconnected"
                error = e.message ?: "Connection failed"
                false
            }
            loading = false
            return connected
        }
        val backoff = ReconnectBackoff()
        suspend fun maintainConnection() {
            var connected = false
            var delayMillis = 0L
            while (true) {
                if (delayMillis > 0) {
                    compositorEventRefreshSignals.awaitRefresh(delayMillis, connected)
                }
                connected = refresh()
                if (connected) {
                    backoff.reset()
                    delayMillis = refreshInterval.delayMillis(selected)
                } else {
                    delayMillis = backoff.nextDelayMillis()
                }
            }
        }
        if (lifecycle == null) maintainConnection()
        else lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) { maintainConnection() }
    }
    LaunchedEffect(token, endpoint, pin) {
        val savedToken = token ?: return@LaunchedEffect
        if (endpoint.isBlank() || pin.isBlank()) return@LaunchedEffect

        suspend fun refreshSystemControls() {
            suspend fun requestState(client: WayControlClient, operation: String): JSONObject = try {
                client.request(operation).put("available", true)
            } catch (exception: CancellationException) {
                throw exception
            } catch (_: Exception) {
                JSONObject().put("available", false)
            }

            val (audio, media) = try {
                withContext(Dispatchers.IO) {
                    val client = WayControlClient(endpoint, pin, savedToken)
                    requestState(client, "audio.info") to requestState(client, "media.info")
                }
            } catch (exception: CancellationException) {
                throw exception
            } catch (_: Exception) {
                JSONObject().put("available", false) to JSONObject().put("available", false)
            }
            snapshot = snapshot.copy(audio = audio, media = media)
        }

        suspend fun maintainSystemControls() {
            while (true) {
                if (shouldRefreshSystemControls(currentDestination)) refreshSystemControls()
                delay(SYSTEM_CONTROLS_REFRESH_INTERVAL_MILLIS)
            }
        }

        if (lifecycle == null) maintainSystemControls()
        else lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) { maintainSystemControls() }
    }
    LaunchedEffect(selected, artworkTarget, token, endpoint, pin) {
        val (playerId, artworkId) = artworkTarget ?: run { mediaArtwork = null; return@LaunchedEffect }
        if (selected != Destination.System || token == null || endpoint.isBlank() || pin.isBlank()) {
            mediaArtwork = null
            return@LaunchedEffect
        }
        mediaArtwork = null
        mediaArtwork = withContext(Dispatchers.IO) {
            runCatching {
                val response = WayControlClient(endpoint, pin, token).request(
                    "media.artwork",
                    JSONObject().put("player_id", playerId).put("artwork_id", artworkId),
                )
                decodeMediaArtwork(response)?.takeIf { it.artworkId == artworkId }
            }.getOrNull()
        }
    }
    LaunchedEffect(token, endpoint, pin, capabilities.events) {
        val savedToken = token ?: return@LaunchedEffect
        if (endpoint.isBlank() || pin.isBlank() || !capabilities.events) return@LaunchedEffect
        val backoff = ReconnectBackoff()
        suspend fun subscribe(onConnected: () -> Unit) {
            val parentContext = currentCoroutineContext()
            withContext(Dispatchers.IO) {
                WayControlClient(endpoint, pin, savedToken).readEvents(
                    shouldStop = { !parentContext.isActive },
                    onConnected = onConnected,
                    onEvent = { compositorEventRefreshSignals.signal() },
                )
            }
        }
        suspend fun maintainEventSubscription() {
            while (true) {
                var connectedAtMillis: Long? = null
                try {
                    subscribe { connectedAtMillis = System.nanoTime() / 1_000_000L }
                } catch (exception: CancellationException) {
                    throw exception
                } catch (_: Exception) {
                    // Retry below with a bounded delay while preserving cancellation.
                } finally {
                    connectedAtMillis?.let { connectedAt ->
                        val durationMillis = (System.nanoTime() / 1_000_000L - connectedAt).coerceAtLeast(0)
                        backoff.resetAfterStableConnection(durationMillis)
                    }
                }
                delay(backoff.nextDelayMillis())
            }
        }
        if (lifecycle == null) maintainEventSubscription()
        else lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) { maintainEventSubscription() }
    }

    MaterialTheme(colorScheme = darkColorScheme(
        primary = Accent, onPrimary = MainText, background = Background, onBackground = MainText,
        surface = SurfaceColor, onSurface = MainText, secondary = Color(0xFFB9567B)
    )) {
        Scaffold(containerColor = Background, contentWindowInsets = WindowInsets.safeDrawing,
            bottomBar = {
                if (showWelcome == false) {
                NavigationBar(containerColor = Background, windowInsets = WindowInsets.navigationBars) {
                    destinations.forEach { destination ->
                        NavigationBarItem(selected = selected == destination, onClick = { selected = destination },
                            icon = { Text(destination.glyph, style = MaterialTheme.typography.titleMedium) },
                            label = { Text(destination.label, maxLines = 1) })
                    }
                }
                }
            }) { insets ->
            Column(Modifier.fillMaxSize().padding(insets).verticalScroll(contentScrollState)
                .padding(horizontal = 20.dp, vertical = 16.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
                if (showWelcome == null) {
                    Header("WayControl", "Restoring secure connection…")
                } else when (selected) {
                    Destination.Home -> HomeScreen(
                        status, loading, error, snapshot.desktop, snapshot.workspaces, snapshot.windows,
                        snapshot.networkLatencyMs, capabilities.switchWorkspace, capabilities,
                        snapshot.monitors, snapshot.audio, snapshot.media,
                        action = { operation, payload ->
                            scope.launch { runCatching { withContext(Dispatchers.IO) { WayControlClient(endpoint, pin, token).request(operation, payload) } }.onFailure { error = it.message } }
                        },
                    ) { id ->
                        scope.launch { runCatching { withContext(Dispatchers.IO) { WayControlClient(endpoint, pin, token).request("workspace.switch", JSONObject().put("workspace_id", id)) } }.onFailure { error = it.message } }
                    }
                    Destination.Workspaces -> WorkspaceScreen(snapshot.workspaces, status, capabilities.switchWorkspace) { id ->
                        scope.launch { runCatching { withContext(Dispatchers.IO) { WayControlClient(endpoint, pin, token).request("workspace.switch", JSONObject().put("workspace_id", id)) } }.onFailure { error = it.message } }
                    }
                    Destination.Windows -> WindowScreen(snapshot.windows, snapshot.workspaces, snapshot.monitors, status, capabilities) { operation, address, payload ->
                        scope.launch { runCatching { withContext(Dispatchers.IO) { WayControlClient(endpoint, pin, token).request(operation, payload.put("address", address)) } }.onFailure { error = it.message } }
                    }
                    Destination.System -> SystemScreenWithHistory(snapshot.desktop, snapshot.monitors, status, capabilities, snapshot.audio, snapshot.media, telemetryHistory, mediaArtwork) { operation, payload ->
                        scope.launch { runCatching { withContext(Dispatchers.IO) { WayControlClient(endpoint, pin, token).request(operation, payload) } }.onFailure { error = it.message } }
                    }
                    Destination.Automation -> AutomationScreen(
                        profiles = automationProfiles,
                        result = automationResult,
                        connected = token != null,
                        onSave = { profile ->
                            val updated = if (automationProfiles.any { it.id == profile.id }) {
                                automationProfiles.map { if (it.id == profile.id) profile else it }
                            } else automationProfiles + profile
                            automationProfiles = updated
                            automationStore.save(updated)
                        },
                        onDelete = { profile ->
                            val updated = automationProfiles.filterNot { it.id == profile.id }
                            automationProfiles = updated
                            automationStore.save(updated)
                            if (automationResult?.optString("profile_id") == profile.id) automationResult = null
                        },
                        onRun = { profile ->
                            if (profileRequiresConfirmation(profile)) profileConfirmation = profile
                            else runAutomation(profile, confirmed = true)
                        },
                    )
                    Destination.Settings -> SettingsScreen(
                        endpoint, { endpoint = it }, pin, { pin = it }, code, { code = it }, status, error, loading,
                        refreshInterval = refreshInterval,
                        onRefreshIntervalChange = { next ->
                            refreshInterval = next
                            telemetryPreferences.edit().putInt("refresh_interval_seconds", next.seconds).apply()
                        },
                        discoveredServers = discoveredServers,
                        discoveryStatus = discoveryStatus,
                        discovering = discovering,
                        scanningQr = scanningQr,
                        onScanQr = {
                            scanningQr = true
                            val options = GmsBarcodeScannerOptions.Builder()
                                .setBarcodeFormats(Barcode.FORMAT_QR_CODE)
                                .enableAutoZoom()
                                .build()
                            GmsBarcodeScanning.getClient(context, options).startScan()
                                .addOnSuccessListener { barcode ->
                                    scanningQr = false
                                    runCatching { PairingQrPayload.decode(barcode.rawValue.orEmpty()) }
                                        .onSuccess { details ->
                                            endpoint = details.endpoint
                                            pin = details.fingerprint
                                            code = details.code
                                            val remaining = (details.expiresAtUnix - System.currentTimeMillis() / 1000).coerceAtLeast(0)
                                            status = "QR pairing details ready · ${remaining}s remaining"
                                            error = null
                                        }
                                        .onFailure { exception ->
                                            status = "QR code rejected"
                                            error = exception.message ?: "Pairing QR is invalid"
                                        }
                                }
                                .addOnCanceledListener { scanningQr = false }
                                .addOnFailureListener { exception ->
                                    scanningQr = false
                                    status = "QR scanning unavailable"
                                    error = "Update Google Play services or enter the pairing details manually. ${exception.message.orEmpty()}".trim()
                                }
                        },
                        onDiscover = { startDiscovery() },
                        onPair = {
                            loading = true; error = null; status = "Waiting for Linux approval"
                            scope.launch {
                                try {
                                    val client = withContext(Dispatchers.IO) { WayControlClient(endpoint, pin) }
                                    val pair = withContext(Dispatchers.IO) { client.beginPairing(code.trim(), "Android device") }
                                    val deadline = System.currentTimeMillis() + 300_000
                                    var approvedToken: String? = null
                                    while (System.currentTimeMillis() < deadline && approvedToken == null) {
                                        delay(pair.pollAfterMs.coerceIn(300, 5000))
                                        val reply = withContext(Dispatchers.IO) { client.pairingStatus(pair.requestId) }
                                        if (reply.optString("status") == "approved") approvedToken = reply.optString("token").takeIf { it.isNotBlank() }
                                    }
                                    check(approvedToken != null) { "Approval timed out. Request a fresh pairing code." }
                                    store.save(endpoint, pin, approvedToken!!)
                                    markWelcomeCompleted()
                                    selected = Destination.Home
                                    token = approvedToken; status = "Connected"; error = null
                                } catch (e: Exception) { status = "Pairing failed"; error = e.message ?: "Pairing failed" }
                                finally { loading = false }
                            }
                        }, onDisconnect = {
                            markWelcomeCompleted()
                            store.clear(); token = null; snapshot = snapshot.cleared(); status = "Disconnected"; endpoint = ""; pin = ""; error = null
                        }, store = store, setToken = { token = it }, setStatus = { status = it }, setError = { error = it }, setLoading = { loading = it },
                        welcome = showWelcome == true,
                        onSkipWelcome = {
                            markWelcomeCompleted()
                            selected = Destination.Home
                        },
                    )
                }
                if (error != null && selected != Destination.Settings) Text(error!!, color = Color(0xFFE6A6B8))
            }
        }
    }
    profileConfirmation?.let { profile ->
        AlertDialog(
            onDismissRequest = { profileConfirmation = null },
            title = { Text("Run ${profile.name}?") },
            text = { Text("This profile includes actions that need your confirmation. Review its ordered steps before continuing.") },
            confirmButton = {
                Button(onClick = {
                    profileConfirmation = null
                    runAutomation(profile, confirmed = true)
                }) { Text("Run profile") }
            },
            dismissButton = { Button(onClick = { profileConfirmation = null }) { Text("Cancel") } },
        )
    }
}

@Composable internal fun HomeScreen(
    status: String,
    loading: Boolean,
    error: String?,
    desktop: JSONObject,
    workspaces: JSONArray,
    windows: JSONArray,
    networkLatencyMs: Long?,
    canSwitchWorkspace: Boolean,
    capabilities: CompositorCapabilities = CompositorCapabilities(),
    monitors: JSONArray = JSONArray(),
    audio: JSONObject = JSONObject(),
    media: JSONObject = JSONObject(),
    action: (String, JSONObject) -> Unit = { _, _ -> },
    switchWorkspace: (String) -> Unit,
) {
    val summary = homeDashboardSummary(desktop, workspaces, windows, networkLatencyMs)
    Header("WayControl", summary.compositor)
    Card {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(summary.computerName, color = MainText, style = MaterialTheme.typography.titleMedium)
                Text("${summary.operatingSystem} · ${summary.compositor}", color = SecondaryText, style = MaterialTheme.typography.bodyMedium)
                Text("${if (loading) "Refreshing" else error ?: status} · ${summary.networkLatency}", color = SecondaryText, style = MaterialTheme.typography.bodyMedium)
            }
            Text(status.uppercase(), color = if (status == "Connected") Color(0xFF85C99A) else SecondaryText, style = MaterialTheme.typography.labelMedium)
        }
    }
    Card {
        Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text("Active workspace", color = SecondaryText)
            Text(summary.activeWorkspace, color = MainText, fontWeight = FontWeight.Medium)
            Text("Active application", color = SecondaryText)
            Text(summary.activeApplication, color = MainText, fontWeight = FontWeight.Medium)
        }
    }
    Card {
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("Switch workspace", color = MainText, style = MaterialTheme.typography.titleSmall)
            if (workspaces.length() == 0) {
                Text("No workspace data", color = SecondaryText)
            } else {
                Row(
                    modifier = Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    for (index in 0 until workspaces.length()) {
                        val workspace = workspaces.optJSONObject(index) ?: continue
                        val id = workspace.optString("id")
                        val name = workspace.optString("name").takeIf(String::isNotBlank) ?: id
                        Button(
                            onClick = { switchWorkspace(id) },
                            enabled = canSwitchWorkspace && !workspace.optBoolean("active"),
                        ) { Text(name) }
                    }
                }
                if (!canSwitchWorkspace) Text("Read only", color = SecondaryText, style = MaterialTheme.typography.bodySmall)
            }
        }
    }
    HomeQuickActions(capabilities, monitors, audio, media, action)
    Text("Performance", color = MainText, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Medium)
    Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Metric("CPU", desktop.optDouble("cpu_usage_percent", Double.NaN).let { if (it.isNaN()) "Unavailable" else "%.1f%%".format(it) }, Modifier.weight(1f))
        val memory = desktop.optJSONObject("memory")
        Metric("Memory", memory?.let { "${bytes(it.optLong("used_bytes"))} / ${bytes(it.optLong("total_bytes"))}" } ?: "Unavailable", Modifier.weight(1f))
    }
    Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Metric("GPU", gpuSummary(desktop), Modifier.weight(1f))
        Metric("Network", networkThroughput(desktop), Modifier.weight(1f))
    }
    Metric("Load average", loadAverageSummary(desktop))
    Metric("Uptime", desktop.optDouble("uptime_seconds", Double.NaN).let { if (it.isNaN()) "Unavailable" else duration(it) })
    Card { Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text("Workspaces", color = MainText, style = MaterialTheme.typography.titleSmall)
        Text(if (workspaces.length() == 0) "No workspace data" else "${workspaces.length()} workspaces available", color = SecondaryText)
    } }
}

@Composable
private fun HomeQuickActions(
    capabilities: CompositorCapabilities,
    monitors: JSONArray,
    audio: JSONObject,
    media: JSONObject,
    action: (String, JSONObject) -> Unit,
) {
    var confirmation by remember { mutableStateOf<Pair<String, String>?>(null) }
    Card {
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("Quick actions", color = MainText, style = MaterialTheme.typography.titleSmall)
            if (capabilities.lockScreen || capabilities.monitorPower) Row(
                modifier = Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                if (capabilities.lockScreen) Button(onClick = { action("system.lock", JSONObject()) }) { Text("Lock") }
                if (capabilities.monitorPower) for (index in 0 until monitors.length()) {
                    val monitor = monitors.optJSONObject(index) ?: continue
                    val name = monitor.optString("name")
                    val enabled = monitor.optBoolean("dpms_enabled", true)
                    Button(onClick = {
                        if (enabled) confirmation = "monitor" to name
                        else action("monitor.power", JSONObject().put("monitor", name).put("enabled", true))
                    }) { Text(if (enabled) "Turn off $name" else "Turn on $name") }
                }
            }
            if (audio.optBoolean("available")) {
                Row(
                    modifier = Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Button(onClick = { action("audio.volume", JSONObject().put("direction", "down")) }) { Text("Volume −") }
                    Button(onClick = { action("audio.volume", JSONObject().put("direction", "up")) }) { Text("Volume +") }
                    val muted = if (audio.has("output_muted") && !audio.isNull("output_muted")) audio.optBoolean("output_muted") else null
                    Button(onClick = { action("audio.output_mute", JSONObject()) }) {
                        Text(when (muted) { true -> "Unmute"; false -> "Mute"; null -> "Mute toggle" })
                    }
                    val microphoneMuted = if (audio.has("microphone_muted") && !audio.isNull("microphone_muted")) audio.optBoolean("microphone_muted") else null
                    Button(onClick = { action("audio.microphone_mute", JSONObject()) }) {
                        Text(when (microphoneMuted) { true -> "Unmute mic"; false -> "Mute mic"; null -> "Mic mute toggle" })
                    }
                }
            } else {
                Text("Audio controls unavailable", color = SecondaryText)
            }
            if (media.optBoolean("available")) {
                val players = media.optJSONArray("players") ?: JSONArray()
                val player = players.optJSONObject(0)
                if (player == null) {
                    Button(onClick = { action("media.play_pause", JSONObject()) }) { Text("Play / pause") }
                } else if (mediaPlayerPresentation(player).showPlayPause) {
                    Button(onClick = {
                        action("media.play_pause_for", JSONObject().put("player_id", player.optString("player_id")))
                    }) { Text(mediaPlayerPresentation(player).playPauseLabel) }
                }
            } else {
                Text("Media controls unavailable", color = SecondaryText)
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Button(onClick = { confirmation = "power" to "suspend" }) { Text("Suspend") }
                Button(onClick = { confirmation = "power" to "reboot" }) { Text("Reboot") }
                Button(onClick = { confirmation = "power" to "shutdown" }) { Text("Shut down") }
            }
        }
    }
    confirmation?.let { (kind, value) ->
        val label = if (kind == "monitor") "Turn off" else value.replaceFirstChar { it.uppercase() }
        AlertDialog(
            onDismissRequest = { confirmation = null },
            title = { Text(if (kind == "monitor") "Turn off $value?" else "$label this computer?") },
            text = {
                Text(if (kind == "monitor") "The display will remain off until it is turned back on or the session wakes it." else "This will interrupt desktop activity. Confirm only if you intend to continue.")
            },
            confirmButton = {
                Button(onClick = {
                    if (kind == "monitor") action("monitor.power", JSONObject().put("monitor", value).put("enabled", false))
                    else action("system.power", JSONObject().put("action", value))
                    confirmation = null
                }) { Text(if (kind == "monitor") label else "Confirm $label") }
            },
            dismissButton = { Button(onClick = { confirmation = null }) { Text("Cancel") } },
        )
    }
}

@Composable internal fun WorkspaceScreen(items: JSONArray, status: String, canSwitch: Boolean, switch: (String) -> Unit) {
    Header("Workspaces", status)
    var workspaceId by rememberSaveable { mutableStateOf("") }
    if (canSwitch) {
        OutlinedTextField(
            value = workspaceId,
            onValueChange = { workspaceId = it },
            modifier = Modifier.fillMaxWidth(),
            label = { Text("Workspace ID") },
            placeholder = { Text("Enter a workspace ID") },
            singleLine = true,
        )
        Button(
            onClick = { switch(workspaceId.trim()); workspaceId = "" },
            enabled = workspaceId.isNotBlank(),
        ) { Text("Go to workspace") }
    }
    if (items.length() == 0) Empty("No workspace data", status)
    for (i in 0 until items.length()) {
        val item = items.optJSONObject(i) ?: continue
        val id = item.optString("id")
        Card(if (canSwitch) Modifier.clickable { switch(id) } else Modifier) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                Column { Text(item.optString("name", id), color = MainText, fontWeight = FontWeight.Medium); Text("${item.optLong("window_count")} windows · ${item.optString("monitor", "Monitor unavailable")}", color = SecondaryText) }
                if (item.optBoolean("active")) Text("ACTIVE", color = Color(0xFFB9567B)) else if (canSwitch) Text("Switch", color = SecondaryText) else Text("Read only", color = SecondaryText)
            }
        }
    }
}

@Composable private fun WindowScreen(items: JSONArray, workspaces: JSONArray, monitors: JSONArray, status: String, capabilities: CompositorCapabilities, action: (String, String, JSONObject) -> Unit) {
    Header("Windows", status)
    var searchQuery by rememberSaveable { mutableStateOf("") }
    var groupingName by rememberSaveable { mutableStateOf(WindowGrouping.WORKSPACE.name) }
    val grouping = if (groupingName == WindowGrouping.MONITOR.name) WindowGrouping.MONITOR else WindowGrouping.WORKSPACE
    var groupingMenuOpen by remember { mutableStateOf(false) }
    OutlinedTextField(
        value = searchQuery,
        onValueChange = { searchQuery = it },
        modifier = Modifier.fillMaxWidth(),
        label = { Text("Search windows") },
        placeholder = { Text("Title, application, workspace, monitor") },
        singleLine = true,
    )
    Box {
        Button(onClick = { groupingMenuOpen = true }) {
            Text("Group by ${if (grouping == WindowGrouping.WORKSPACE) "workspace" else "monitor"}")
        }
        DropdownMenu(expanded = groupingMenuOpen, onDismissRequest = { groupingMenuOpen = false }) {
            DropdownMenuItem(
                text = { Text("Workspace") },
                onClick = { groupingName = WindowGrouping.WORKSPACE.name; groupingMenuOpen = false },
            )
            DropdownMenuItem(
                text = { Text("Monitor") },
                onClick = { groupingName = WindowGrouping.MONITOR.name; groupingMenuOpen = false },
            )
        }
    }
    val groups = groupFilteredWindows(items, workspaces, searchQuery, grouping)
    if (items.length() == 0) Empty("No open windows", status)
    else if (groups.isEmpty()) Empty("No matching windows", status)
    var closeTarget by remember { mutableStateOf<String?>(null) }
    for (group in groups) {
        Text(group.label, color = SecondaryText, style = MaterialTheme.typography.titleSmall)
        for (item in group.windows) {
            val address = item.optString("address")
            val iconData = item.optString("icon_png_base64").takeIf(String::isNotBlank)
            val iconBitmap by produceState<Bitmap?>(null, iconData) {
                value = withContext(Dispatchers.Default) { iconData?.let(::decodeWindowIcon) }
            }
            val bitmap = iconBitmap
            var menuOpen by remember(address) { mutableStateOf(false) }
            var monitorMenuOpen by remember(address) { mutableStateOf(false) }
            Card {
                Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Row(horizontalArrangement = Arrangement.spacedBy(10.dp), verticalAlignment = Alignment.CenterVertically) {
                        if (bitmap != null) {
                            Image(
                                bitmap = bitmap.asImageBitmap(),
                                contentDescription = null,
                                modifier = Modifier.size(32.dp).clip(RoundedCornerShape(8.dp)),
                            )
                        }
                        Column {
                            Text(item.optString("title", "Untitled window"), color = MainText, fontWeight = FontWeight.Medium)
                            Text(item.optString("app_id", "Unknown app"), color = SecondaryText)
                        }
                    }
                    val monitorName = item.optString("monitor").takeIf { it.isNotBlank() && it != "null" } ?: "—"
                    Text("workspace ${item.optString("workspace_id", "—")} · $monitorName", color = SecondaryText)
                    if (item.optBoolean("focused")) Text("FOCUSED", color = Color(0xFFB9567B), style = MaterialTheme.typography.labelSmall)
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        if (capabilities.focusWindow) Button(onClick = { action("window.focus", address, JSONObject()) }) { Text("Focus") }
                        if (capabilities.toggleFloating) Button(onClick = { action("window.toggle_floating", address, JSONObject()) }) { Text(if (item.optBoolean("floating")) "Tile" else "Float") }
                        if (capabilities.toggleFullscreen) Button(onClick = { action("window.toggle_fullscreen", address, JSONObject()) }) { Text(if (item.optBoolean("fullscreen")) "Exit full screen" else "Full screen") }
                    }
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        if (capabilities.togglePinned && item.optBoolean("floating") && item.has("pinned") && !item.isNull("pinned")) Button(onClick = { action("window.toggle_pinned", address, JSONObject()) }) { Text(if (item.optBoolean("pinned")) "Unpin" else "Pin") }
                        if (capabilities.centerFloatingWindow && item.optBoolean("floating")) Button(onClick = { action("window.center_floating", address, JSONObject()) }) { Text("Center") }
                        if (capabilities.moveWindow && capabilities.workspaces) Button(onClick = { menuOpen = true }) { Text("Move to workspace") }
                        DropdownMenu(expanded = menuOpen && capabilities.moveWindow && capabilities.workspaces, onDismissRequest = { menuOpen = false }) {
                            for (w in 0 until workspaces.length()) {
                                val workspace = workspaces.optJSONObject(w) ?: continue
                                val id = workspace.optString("id")
                                DropdownMenuItem(
                                    text = { Text(workspace.optString("name", id)) },
                                    onClick = { menuOpen = false; action("window.move", address, JSONObject().put("workspace_id", id)) },
                                )
                            }
                        }
                    }
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        if (capabilities.moveWindowToMonitor && capabilities.monitors) Button(onClick = { monitorMenuOpen = true }) { Text("Move to monitor") }
                        DropdownMenu(expanded = monitorMenuOpen && capabilities.moveWindowToMonitor && capabilities.monitors, onDismissRequest = { monitorMenuOpen = false }) {
                            for (m in 0 until monitors.length()) {
                                val monitor = monitors.optJSONObject(m) ?: continue
                                val name = monitor.optString("name").takeIf { it.isNotBlank() } ?: continue
                                DropdownMenuItem(
                                    text = { Text(name) },
                                    onClick = { monitorMenuOpen = false; action("window.move_monitor", address, JSONObject().put("monitor", name)) },
                                )
                            }
                        }
                        if (capabilities.closeWindow) Button(onClick = { closeTarget = address }) { Text("Close") }
                    }
                }
            }
        }
    }
    closeTarget?.let { target ->
        AlertDialog(
            onDismissRequest = { closeTarget = null },
            title = { Text("Close this window?") },
            text = { Text("The app may ask whether to save its work before closing.") },
            confirmButton = { Button(onClick = { action("window.close", target, JSONObject()); closeTarget = null }) { Text("Close window") } },
            dismissButton = { Button(onClick = { closeTarget = null }) { Text("Cancel") } }
        )
    }
}

@Composable internal fun SystemScreen(desktop: JSONObject, monitors: JSONArray, status: String, capabilities: CompositorCapabilities = CompositorCapabilities(), audio: JSONObject = JSONObject(), media: JSONObject = JSONObject(), artwork: MediaArtworkImage? = null, action: (String, JSONObject) -> Unit) {
    SystemScreenContent(desktop, monitors, status, capabilities, audio, media, TelemetryHistory(), artwork, action)
}

@Composable internal fun SystemScreenWithHistory(desktop: JSONObject, monitors: JSONArray, status: String, capabilities: CompositorCapabilities = CompositorCapabilities(), audio: JSONObject = JSONObject(), media: JSONObject = JSONObject(), history: TelemetryHistory, artwork: MediaArtworkImage? = null, action: (String, JSONObject) -> Unit) {
    SystemScreenContent(desktop, monitors, status, capabilities, audio, media, history, artwork, action)
}

@Composable private fun SystemScreenContent(desktop: JSONObject, monitors: JSONArray, status: String, capabilities: CompositorCapabilities, audio: JSONObject, media: JSONObject, history: TelemetryHistory, artwork: MediaArtworkImage?, action: (String, JSONObject) -> Unit) {
    Header("System monitor", status)
    TelemetryHistoryCharts(history)
    var powerConfirmation by remember { mutableStateOf<String?>(null) }
    var monitorConfirmation by remember { mutableStateOf<String?>(null) }
    val memory = desktop.optJSONObject("memory")
    val metrics = listOf(
        "CPU utilization" to desktop.optDouble("cpu_usage_percent", Double.NaN).let { if (it.isNaN()) "Unavailable" else "%.1f%%".format(it) },
        "CPU frequency" to desktop.optLong("cpu_frequency_khz", -1).let { if (it < 0) "Unavailable" else "${it / 1000} MHz" }, "Memory used" to (memory?.let { bytes(it.optLong("used_bytes")) } ?: "Unavailable"),
        "Memory total" to (memory?.let { bytes(it.optLong("total_bytes")) } ?: "Unavailable"),
        "Swap used" to (memory?.let { bytes(it.optLong("swap_used_bytes")) } ?: "Unavailable"),
        "NVIDIA GPU" to gpuSummary(desktop), "GPU temperature" to gpuTemperature(desktop),
        "GPU VRAM" to gpuMemory(desktop), "GPU power" to gpuPower(desktop),
        "Network throughput" to networkThroughput(desktop), "Load average" to loadAverageSummary(desktop),
        "Uptime" to desktop.optDouble("uptime_seconds", Double.NaN).let { if (it.isNaN()) "Unavailable" else duration(it) }
    ) + gpuOptionalMetricRows(desktop.optJSONObject("gpu")).map { it.label to it.value }
    metrics.forEach { (title, value) ->
        Card {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                Text(title, color = SecondaryText, modifier = Modifier.weight(1f))
                Text(value, color = MainText, modifier = Modifier.weight(1f), textAlign = TextAlign.End)
            }
        }
    }
    val storage = storageRows(desktop.optJSONArray("storage") ?: JSONArray())
    if (storage.isNotEmpty()) {
        Card {
            Text("Storage", color = MainText, fontWeight = FontWeight.Medium)
            storage.forEach { mount ->
                Column(Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
                    Text(mount.mountPoint, color = MainText)
                    Text(mount.sourceAndType, color = SecondaryText, style = MaterialTheme.typography.bodySmall)
                    Text(mount.usage, color = SecondaryText, style = MaterialTheme.typography.bodySmall)
                }
            }
        }
    }
    val networkInterfaces = networkInterfaceRows(
        desktop.optJSONArray("network_interfaces") ?: JSONArray(),
        desktop.optJSONArray("network_rates") ?: JSONArray(),
    )
    if (networkInterfaces.isNotEmpty()) {
        Card {
            Text("Network interfaces", color = MainText, fontWeight = FontWeight.Medium)
            networkInterfaces.forEach { network ->
                Column(Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                        Text(network.name, color = MainText)
                        Text(network.linkState, color = SecondaryText)
                    }
                    Text(network.addresses, color = SecondaryText, style = MaterialTheme.typography.bodySmall)
                    val receive = network.receivedBytesPerSecond?.let(::rate) ?: "Unavailable"
                    val transmit = network.transmittedBytesPerSecond?.let(::rate) ?: "Unavailable"
                    Text("↓ $receive · ↑ $transmit", color = SecondaryText, style = MaterialTheme.typography.bodySmall)
                }
            }
        }
    }
    val cpuCores = desktop.optJSONArray("cpu_cores") ?: JSONArray()
    if (cpuCores.length() > 0) {
        Card {
            Text("CPU cores", color = MainText, fontWeight = FontWeight.Medium)
            for (index in 0 until minOf(cpuCores.length(), 32)) {
                val core = cpuCores.optJSONObject(index) ?: continue
                val usage = core.optDouble("usage_percent", Double.NaN)
                val usageText = if (usage.isFinite()) "${"%.1f".format(usage)}%" else "Unavailable"
                val frequency = core.optLong("frequency_khz", -1)
                val value = if (frequency > 0) "$usageText · ${frequency / 1000} MHz" else usageText
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                    Text(core.optString("id", "CPU core $index"), color = SecondaryText)
                    Text(value, color = MainText)
                }
            }
            if (cpuCores.length() > 32) Text("${cpuCores.length() - 32} more cores", color = SecondaryText)
        }
    }
    val coreTemperatures = cpuCoreTemperatureRows(
        desktop.optJSONArray("cpu_core_temperatures") ?: JSONArray(),
    )
    if (coreTemperatures.isNotEmpty()) {
        Card {
            Text("CPU core temperatures", color = MainText, fontWeight = FontWeight.Medium)
            coreTemperatures.forEach { temperature ->
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                    Text(temperature.label, color = SecondaryText)
                    Text(temperature.temperature, color = MainText)
                }
            }
        }
    }

    Text("Controls", color = MainText, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Medium)
    Card {
        Text("Audio and media", color = MainText, fontWeight = FontWeight.Medium)
        if (audio.optBoolean("available")) {
            val outputDevices = audio.optJSONArray("output_devices") ?: JSONArray()
            val inputDevices = audio.optJSONArray("input_devices") ?: JSONArray()
            if (outputDevices.length() > 0) {
                var outputMenuExpanded by remember { mutableStateOf(false) }
                val activeOutput = (0 until outputDevices.length()).mapNotNull { outputDevices.optJSONObject(it) }
                    .firstOrNull { it.optBoolean("active") }
                Box {
                    Button(onClick = { outputMenuExpanded = true }) {
                        Text("Output: ${activeOutput?.optString("name") ?: "Select device"}")
                    }
                    DropdownMenu(expanded = outputMenuExpanded, onDismissRequest = { outputMenuExpanded = false }) {
                        for (index in 0 until outputDevices.length()) {
                            val device = outputDevices.optJSONObject(index) ?: continue
                            DropdownMenuItem(
                                text = { Text(device.optString("name")) },
                                onClick = {
                                    outputMenuExpanded = false
                                    action("audio.output.select", JSONObject().put("id", device.optLong("id")))
                                },
                            )
                        }
                    }
                }
            }
            if (inputDevices.length() > 0) {
                var inputMenuExpanded by remember { mutableStateOf(false) }
                val activeInput = (0 until inputDevices.length()).mapNotNull { inputDevices.optJSONObject(it) }
                    .firstOrNull { it.optBoolean("active") }
                Box {
                    Button(onClick = { inputMenuExpanded = true }) {
                        Text("Input: ${activeInput?.optString("name") ?: "Select device"}")
                    }
                    DropdownMenu(expanded = inputMenuExpanded, onDismissRequest = { inputMenuExpanded = false }) {
                        for (index in 0 until inputDevices.length()) {
                            val device = inputDevices.optJSONObject(index) ?: continue
                            DropdownMenuItem(
                                text = { Text(device.optString("name")) },
                                onClick = {
                                    inputMenuExpanded = false
                                    action("audio.input.select", JSONObject().put("id", device.optLong("id")))
                                },
                            )
                        }
                    }
                }
            }
            val initialVolume = audio.optInt("output_volume_percent", 0).coerceIn(0, 100).toFloat()
            var outputVolume by remember(initialVolume) { mutableFloatStateOf(initialVolume) }
            Text("Output volume · ${outputVolume.roundToInt()}%", color = SecondaryText)
            Slider(
                modifier = Modifier.semantics { contentDescription = "Output volume" },
                value = outputVolume,
                onValueChange = { outputVolume = it.coerceIn(0f, 100f) },
                onValueChangeFinished = {
                    action("audio.volume.set", JSONObject().put("percent", outputVolume.roundToInt()))
                },
                valueRange = 0f..100f,
            )
            val outputMuted = if (audio.has("output_muted") && !audio.isNull("output_muted")) {
                audio.optBoolean("output_muted")
            } else null
            Text(
                when (outputMuted) {
                    true -> "Output muted"
                    false -> "Output active"
                    null -> "Output mute state unavailable"
                },
                color = SecondaryText,
            )
            val microphoneMuted = if (audio.has("microphone_muted") && !audio.isNull("microphone_muted")) {
                audio.optBoolean("microphone_muted")
            } else null
            Text(
                when (microphoneMuted) {
                    true -> "Microphone muted"
                    false -> "Microphone active"
                    null -> "Microphone mute state unavailable"
                },
                color = SecondaryText,
            )
        } else {
            Text("Output volume unavailable", color = SecondaryText)
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(onClick = { action("audio.volume", JSONObject().put("direction", "down")) }) { Text("Volume −") }
            Button(onClick = { action("audio.volume", JSONObject().put("direction", "up")) }) { Text("Volume +") }
            val outputMuted = if (audio.has("output_muted") && !audio.isNull("output_muted")) {
                audio.optBoolean("output_muted")
            } else null
            Button(onClick = { action("audio.output_mute", JSONObject()) }) {
                Text(when (outputMuted) { true -> "Unmute output"; false -> "Mute output"; null -> "Toggle output mute" })
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            val microphoneMuted = if (audio.has("microphone_muted") && !audio.isNull("microphone_muted")) {
                audio.optBoolean("microphone_muted")
            } else null
            Button(onClick = { action("audio.microphone_mute", JSONObject()) }) {
                Text(when (microphoneMuted) { true -> "Unmute microphone"; false -> "Mute microphone"; null -> "Toggle mic mute" })
            }
        }
        val mediaPlayers = media.optJSONArray("players") ?: JSONArray()
        if (media.optBoolean("available") && mediaPlayers.length() > 0) {
            Text("Media players", color = MainText, fontWeight = FontWeight.Medium)
            for (index in 0 until mediaPlayers.length()) {
                val player = mediaPlayers.optJSONObject(index) ?: continue
                val playerId = player.optString("player_id")
                val playerName = player.optString("name", playerId)
                Text(playerName + " · " + player.optString("playback_status", "Unknown"), color = SecondaryText)
                if (player.optString("artwork_id") == artwork?.artworkId) {
                    Image(
                        bitmap = artwork.bitmap.asImageBitmap(),
                        contentDescription = "Album artwork for $playerName",
                        modifier = Modifier.size(112.dp).clip(RoundedCornerShape(12.dp)),
                    )
                }
                val title = player.optString("title")
                val artist = player.optString("artist")
                val album = player.optString("album")
                if (title.isNotBlank()) Text(title, color = MainText, fontWeight = FontWeight.Medium)
                val details = listOf(artist, album).filter { it.isNotBlank() }.joinToString(" · ")
                if (details.isNotBlank()) Text(details, color = SecondaryText)
                val presentation = mediaPlayerPresentation(player)
                presentation.positionText?.let { Text(it, color = SecondaryText) }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (presentation.canGoPrevious) Button(onClick = { action("media.previous", JSONObject().put("player_id", playerId)) }) { Text("Previous") }
                    if (presentation.showPlayPause) Button(onClick = { action("media.play_pause_for", JSONObject().put("player_id", playerId)) }) { Text(presentation.playPauseLabel) }
                    if (presentation.canGoNext) Button(onClick = { action("media.next", JSONObject().put("player_id", playerId)) }) { Text("Next") }
                }
            }
        } else {
            Text("No media players found", color = SecondaryText)
            Button(onClick = { action("media.play_pause", JSONObject()) }) { Text("Play / pause") }
        }
    }
    if (capabilities.lockScreen) Card {
        Text("Session", color = MainText, fontWeight = FontWeight.Medium)
        Button(onClick = { action("system.lock", JSONObject()) }) { Text("Lock screen") }
    }
    Card {
        Text("Monitor power", color = MainText, fontWeight = FontWeight.Medium)
        if (monitors.length() == 0) Text("No monitor data", color = SecondaryText)
        for (i in 0 until monitors.length()) {
            val monitor = monitors.optJSONObject(i) ?: continue
            val name = monitor.optString("name")
            val enabled = monitor.optBoolean("dpms_enabled", true)
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                Column {
                    Text(name, color = MainText)
                    Text(if (enabled) "Display on" else "Display off", color = SecondaryText)
                }
                if (capabilities.monitorPower) Button(onClick = {
                    if (enabled) monitorConfirmation = name
                    else action("monitor.power", JSONObject().put("monitor", name).put("enabled", true))
                }) { Text(if (enabled) "Turn off" else "Turn on") }
            }
        }
    }
    Card {
        Text("Power", color = MainText, fontWeight = FontWeight.Medium)
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(onClick = { powerConfirmation = "suspend" }) { Text("Suspend") }
            Button(onClick = { powerConfirmation = "shutdown" }) { Text("Shut down") }
            Button(onClick = { powerConfirmation = "reboot" }) { Text("Reboot") }
        }
    }
    powerConfirmation?.let { requested ->
        val label = when (requested) { "suspend" -> "Suspend"; "shutdown" -> "Shut down"; else -> "Reboot" }
        AlertDialog(
            onDismissRequest = { powerConfirmation = null },
            title = { Text("$label this computer?") },
            text = { Text("This will interrupt desktop activity. Confirm only if you intend to continue.") },
            confirmButton = { Button(onClick = { action("system.power", JSONObject().put("action", requested)); powerConfirmation = null }) { Text(label) } },
            dismissButton = { Button(onClick = { powerConfirmation = null }) { Text("Cancel") } }
        )
    }
    monitorConfirmation?.let { name ->
        AlertDialog(
            onDismissRequest = { monitorConfirmation = null },
            title = { Text("Turn off $name?") },
            text = { Text("The display will remain off until it is turned back on or the session wakes it.") },
            confirmButton = { Button(onClick = { action("monitor.power", JSONObject().put("monitor", name).put("enabled", false)); monitorConfirmation = null }) { Text("Turn off") } },
            dismissButton = { Button(onClick = { monitorConfirmation = null }) { Text("Cancel") } }
        )
    }
}

@Composable internal fun SettingsScreen(endpoint: String, setEndpoint: (String) -> Unit, pin: String, setPin: (String) -> Unit,
    code: String, setCode: (String) -> Unit, status: String, error: String?, loading: Boolean, onPair: () -> Unit,
    refreshInterval: TelemetryRefreshInterval, onRefreshIntervalChange: (TelemetryRefreshInterval) -> Unit,
    discoveredServers: List<DiscoveredServer>, discoveryStatus: String?, discovering: Boolean, onDiscover: () -> Unit,
    scanningQr: Boolean, onScanQr: () -> Unit,
    onDisconnect: () -> Unit, store: SecureConnectionStore, setToken: (String) -> Unit,
    setStatus: (String) -> Unit, setError: (String?) -> Unit, setLoading: (Boolean) -> Unit,
    welcome: Boolean = false, onSkipWelcome: () -> Unit = {}) {
    if (welcome) {
        Header("Welcome to WayControl", "Securely connect your Linux desktop")
        Text("Find your desktop, verify its certificate, and approve pairing in the Linux terminal.", color = SecondaryText)
        Button(onClick = onSkipWelcome, enabled = !loading) { Text("Continue without pairing") }
    } else {
        Header("Settings", "Secure desktop connection")
        TelemetryRefreshSection(refreshInterval, onRefreshIntervalChange)
    }
    Text("Scan the pairing QR shown by the trusted Linux terminal, or enter the HTTPS address, certificate fingerprint, and short-lived code manually. Approve the request in the Linux terminal.", color = SecondaryText)
    PairingQrSection(scanningQr, onScanQr, enabled = !loading)
    LanDiscoverySection(discoveredServers, discoveryStatus, discovering, onDiscover, setEndpoint)
    OutlinedTextField(endpoint, setEndpoint, Modifier.fillMaxWidth(), label = { Text("Server address") }, placeholder = { Text("https://192.168.1.20:8765") }, singleLine = true)
    OutlinedTextField(pin, setPin, Modifier.fillMaxWidth(), label = { Text("Certificate SHA-256") }, singleLine = true)
    OutlinedTextField(code, setCode, Modifier.fillMaxWidth(), label = { Text("Pairing code") }, singleLine = true)
    Button(onClick = onPair, enabled = !loading && endpoint.isNotBlank() && pin.isNotBlank() && code.isNotBlank()) { Text(if (status == "Waiting for Linux approval") "Waiting for approval…" else "Pair this device") }
    Text("Status: $status", color = SecondaryText)
    if (error != null) Text(error, color = Color(0xFFE6A6B8))
    if (status == "Waiting for Linux approval") CircularProgressIndicator(color = Accent)
    if (store.load() != null) Button(onClick = onDisconnect) { Text("Disconnect and erase saved credentials") }
}

@Composable
internal fun AutomationScreen(
    profiles: List<AutomationProfile>,
    result: JSONObject?,
    connected: Boolean,
    onSave: (AutomationProfile) -> Unit,
    onDelete: (AutomationProfile) -> Unit,
    onRun: (AutomationProfile) -> Unit,
) {
    var editing by remember { mutableStateOf<AutomationProfile?>(null) }
    Header("Automation profiles", "Run ordered, approved desktop actions")
    Text(
        "Profiles stay on this device. Actions call WayControl's typed operations; profile text is never run as a shell command.",
        color = SecondaryText,
    )
    Button(onClick = {
        editing = AutomationProfile(
            id = "custom-${System.currentTimeMillis()}",
            name = "My profile",
            icon = "⚙️",
            actions = emptyList(),
        )
    }) { Text("Add profile") }
    profiles.forEach { profile ->
        Card {
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f)) {
                    Text("${profile.icon}  ${profile.name}", color = MainText, fontWeight = FontWeight.Medium)
                    Text("${profile.actions.size} ordered action(s)", color = SecondaryText)
                }
                Switch(
                    checked = profile.enabled,
                    onCheckedChange = { onSave(profile.copy(enabled = it)) },
                    modifier = Modifier.semantics { contentDescription = "Enable ${profile.name}" },
                )
            }
            if (profile.actions.isEmpty()) {
                Text("No actions configured yet", color = SecondaryText)
            } else {
                profile.actions.forEachIndexed { index, action ->
                    Text("${index + 1}. ${automationActionLabel(action)}", color = SecondaryText)
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Button(onClick = { editing = profile }) { Text("Edit") }
                Button(onClick = { onRun(profile) }, enabled = connected && profileCanRun(profile)) { Text("Run") }
                if (profile.id.startsWith("custom-")) {
                    Button(onClick = { onDelete(profile) }) { Text("Delete") }
                }
            }
            if (result?.optString("profile_id") == profile.id) {
                Text(result.optString("summary"), color = if (result.optBoolean("succeeded")) Accent else Color(0xFFE6A6B8))
                val steps = result.optJSONArray("steps") ?: JSONArray()
                for (index in 0 until steps.length()) {
                    val step = steps.optJSONObject(index) ?: continue
                    Text(
                        "${step.optString("action")}: ${step.optString("message")}",
                        color = if (step.optBoolean("succeeded")) SecondaryText else Color(0xFFE6A6B8),
                    )
                }
            }
        }
    }
    editing?.let { profile ->
        AutomationProfileEditor(
            profile = profile,
            onDismiss = { editing = null },
            onSave = { editedProfile ->
                onSave(editedProfile)
                editing = null
            },
        )
    }
}

@Composable
private fun AutomationProfileEditor(
    profile: AutomationProfile,
    onDismiss: () -> Unit,
    onSave: (AutomationProfile) -> Unit,
) {
    var name by remember(profile.id) { mutableStateOf(profile.name) }
    var icon by remember(profile.id) { mutableStateOf(profile.icon) }
    var requiresConfirmation by remember(profile.id) { mutableStateOf(profile.requiresConfirmation) }
    var selectedType by remember(profile.id) { mutableStateOf(AutomationActionType.LAUNCH_APPLICATION) }
    var value by remember(profile.id) { mutableStateOf("") }
    var expanded by remember { mutableStateOf(false) }
    var validationError by remember { mutableStateOf<String?>(null) }
    val actions = remember(profile.id) { mutableStateListOf(*profile.actions.toTypedArray()) }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Edit profile") },
        text = {
            Column(
                Modifier.fillMaxWidth().heightIn(max = 560.dp).verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                OutlinedTextField(name, { name = it }, label = { Text("Name") }, singleLine = true)
                OutlinedTextField(icon, { icon = it }, label = { Text("Icon") }, singleLine = true)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Checkbox(requiresConfirmation, { requiresConfirmation = it })
                    Text("Ask before running", color = MainText)
                }
                Text("Ordered actions", color = MainText, fontWeight = FontWeight.Medium)
                actions.forEachIndexed { index, action ->
                    Column {
                        Text("${index + 1}. ${automationActionLabel(action)}", color = SecondaryText)
                        Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                            Button(onClick = {
                                if (index > 0) {
                                    val item = actions.removeAt(index)
                                    actions.add(index - 1, item)
                                }
                            }, enabled = index > 0) { Text("↑") }
                            Button(onClick = {
                                if (index < actions.lastIndex) {
                                    val item = actions.removeAt(index)
                                    actions.add(index + 1, item)
                                }
                            }, enabled = index < actions.lastIndex) { Text("↓") }
                            Button(onClick = { actions.removeAt(index) }) { Text("Remove") }
                        }
                    }
                }
                Box {
                    Button(onClick = { expanded = true }) { Text(selectedType.label) }
                    DropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }) {
                        AutomationActionType.entries.forEach { type ->
                            DropdownMenuItem(
                                text = { Text(type.label) },
                                onClick = { selectedType = type; value = ""; expanded = false },
                            )
                        }
                    }
                }
                if (selectedType.needsValue) {
                    OutlinedTextField(
                        value = value,
                        onValueChange = { value = it },
                        label = { Text(when (selectedType) {
                            AutomationActionType.LAUNCH_APPLICATION -> "Desktop application ID"
                            AutomationActionType.SWITCH_WORKSPACE -> "Workspace name or number"
                            AutomationActionType.SET_VOLUME -> "Output volume (0–100)"
                            else -> "Value"
                        }) },
                        singleLine = true,
                    )
                }
                Button(onClick = {
                    val action = sanitizeAutomationAction(AutomationAction(selectedType, value))
                    if (action == null) validationError = "Enter a valid value for this action"
                    else {
                        actions.add(action)
                        value = ""
                        validationError = null
                    }
                }) { Text("Add action") }
                if (validationError != null) Text(validationError!!, color = Color(0xFFE6A6B8))
            }
        },
        confirmButton = {
            Button(onClick = {
                val updated = sanitizeAutomationProfile(
                    profile.copy(name = name, icon = icon, actions = actions.toList(), requiresConfirmation = requiresConfirmation),
                )
                if (updated == null) validationError = "Provide a valid name and icon"
                else onSave(updated)
            }) { Text("Save") }
        },
        dismissButton = { Button(onClick = onDismiss) { Text("Cancel") } },
    )
}

private fun automationActionLabel(action: AutomationAction): String = when (action.type) {
    AutomationActionType.LAUNCH_APPLICATION -> "${action.type.label}: ${action.value}"
    AutomationActionType.SWITCH_WORKSPACE -> "${action.type.label}: ${action.value}"
    AutomationActionType.SET_VOLUME -> "${action.type.label}: ${action.value}%"
    else -> action.type.label
}

@Composable internal fun TelemetryRefreshSection(
    interval: TelemetryRefreshInterval,
    onSelect: (TelemetryRefreshInterval) -> Unit,
) {
    var expanded by rememberSaveable { mutableStateOf(false) }
    Card {
        Text("Telemetry refresh", color = MainText, fontWeight = FontWeight.Medium)
        Text("Home and System refresh at this rate. Other tabs refresh five times less often.", color = SecondaryText)
        Box {
            Button(onClick = { expanded = true }) { Text(interval.label) }
            DropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }) {
                TelemetryRefreshInterval.entries.forEach { option ->
                    DropdownMenuItem(
                        text = { Text(option.label) },
                        onClick = {
                            expanded = false
                            onSelect(option)
                        },
                    )
                }
            }
        }
    }
}

@Composable internal fun PairingQrSection(scanning: Boolean, onScan: () -> Unit, enabled: Boolean = true) {
    Button(onClick = onScan, enabled = enabled && !scanning) {
        Text(if (scanning) "Opening QR scanner…" else "Scan pairing QR")
    }
}

@Composable internal fun LanDiscoverySection(
    servers: List<DiscoveredServer>,
    status: String?,
    discovering: Boolean,
    onDiscover: () -> Unit,
    onSelect: (String) -> Unit,
) {
    Card {
        Text("Find a desktop", color = MainText, fontWeight = FontWeight.Medium)
        Text("Search this local network for WayControl. Discovery does not verify the server certificate; enter the fingerprint shown by the trusted Linux CLI.", color = SecondaryText)
        Button(onClick = onDiscover, enabled = !discovering) {
            Text(if (discovering) "Searching…" else "Find on local network")
        }
        if (status != null) Text(status, color = SecondaryText)
        servers.forEach { server ->
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(server.name, color = MainText)
                    Text(server.endpoint, color = SecondaryText)
                }
                Button(onClick = { onSelect(server.endpoint) }) { Text("Use address") }
            }
        }
    }
}

@Composable private fun Header(title: String, subtitle: String) {
    Text(title, color = MainText, style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold)
    Text(subtitle, color = SecondaryText, style = MaterialTheme.typography.bodyMedium)
}

@Composable private fun Empty(title: String, detail: String) {
    Card { Column(verticalArrangement = Arrangement.spacedBy(6.dp)) { Text(title, color = MainText); Text(detail, color = SecondaryText) } }
}

@Composable private fun Card(modifier: Modifier = Modifier, content: @Composable ColumnScope.() -> Unit) {
    Surface(color = SurfaceColor, shape = MaterialTheme.shapes.medium, modifier = modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp), content = content)
    }
}

@Composable private fun Metric(label: String, value: String, modifier: Modifier = Modifier) {
    Surface(color = Elevated, shape = MaterialTheme.shapes.medium, modifier = modifier) {
        Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(label, color = SecondaryText, style = MaterialTheme.typography.labelLarge)
            Text(value, color = MainText, style = MaterialTheme.typography.titleMedium)
        }
    }
}

internal fun bytes(value: Long): String {
    if (value < 0) return "Unavailable"
    val units = listOf("B", "KiB", "MiB", "GiB", "TiB")
    var amount = value.toDouble(); var unit = 0
    while (amount >= 1024 && unit < units.lastIndex) { amount /= 1024; unit++ }
    return String.format(Locale.ROOT, "%.1f %s", amount, units[unit])
}

private fun duration(seconds: Double): String {
    val total = seconds.toLong().coerceAtLeast(0); val days = total / 86400; val hours = total % 86400 / 3600; val minutes = total % 3600 / 60
    return if (days > 0) "${days}d ${hours}h" else "${hours}h ${minutes}m"
}

private fun networkThroughput(desktop: JSONObject): String {
    val rates = desktop.optJSONArray("network_rates") ?: return "Unavailable"
    var receive = 0.0; var transmit = 0.0
    for (i in 0 until rates.length()) {
        val item = rates.optJSONObject(i) ?: continue
        receive += item.optDouble("received_bytes_per_second", 0.0)
        transmit += item.optDouble("transmitted_bytes_per_second", 0.0)
    }
    return "↓ ${rate(receive)} · ↑ ${rate(transmit)}"
}

private fun gpuSummary(desktop: JSONObject): String {
    val gpu = desktop.optJSONObject("gpu") ?: return "Unavailable"
    if (!gpu.optBoolean("available")) return "Unavailable"
    val load = gpu.optInt("utilization_percent", -1)
    val name = gpu.optString("name", "NVIDIA GPU")
    return if (load < 0) name else "$name · $load%"
}

private fun loadAverageSummary(desktop: JSONObject): String {
    val load = desktop.optJSONObject("load_average") ?: return "Unavailable"
    val averages = listOf("one_minute", "five_minutes", "fifteen_minutes")
        .map { load.optDouble(it, Double.NaN) }
    return if (averages.all { it.isFinite() && it >= 0.0 }) {
        averages.joinToString(" · ") { "%.2f".format(it) }
    } else "Unavailable"
}

private fun gpuTemperature(desktop: JSONObject): String = desktop.optJSONObject("gpu")
    ?.optInt("temperature_celsius", -1)?.takeIf { it >= 0 }?.let { "$it °C" } ?: "Unavailable"

private fun gpuMemory(desktop: JSONObject): String {
    val gpu = desktop.optJSONObject("gpu") ?: return "Unavailable"
    val used = gpu.optLong("vram_used_bytes", -1); val total = gpu.optLong("vram_total_bytes", -1)
    return if (used < 0 || total <= 0) "Unavailable" else "${bytes(used)} / ${bytes(total)}"
}

private fun gpuPower(desktop: JSONObject): String = desktop.optJSONObject("gpu")
    ?.optDouble("power_watts", Double.NaN)?.takeIf { it.isFinite() }?.let { "%.1f W".format(it) } ?: "Unavailable"

private fun rate(bytesPerSecond: Double): String {
    if (!bytesPerSecond.isFinite() || bytesPerSecond < 0) return "Unavailable"
    if (bytesPerSecond < 1024) return "%.0f B/s".format(bytesPerSecond)
    if (bytesPerSecond < 1024 * 1024) return "%.1f KiB/s".format(bytesPerSecond / 1024)
    return "%.1f MiB/s".format(bytesPerSecond / (1024 * 1024))
}
