package dev.waycontrol

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.Alignment
import androidx.compose.ui.graphics.Color
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
    System("System", "◷"), Settings("Settings", "⚙")
}

internal fun visibleDestinations(capabilities: CompositorCapabilities): List<Destination> = buildList {
    add(Destination.Home)
    if (capabilities.workspaces) add(Destination.Workspaces)
    if (capabilities.windows) add(Destination.Windows)
    add(Destination.System)
    add(Destination.Settings)
}

@Composable
private fun WayControlApp(store: SecureConnectionStore) {
    val context = LocalContext.current
    val lifecycle = (context as? LifecycleOwner)?.lifecycle
    val telemetryPreferences = remember(context) {
        context.getSharedPreferences("telemetry_settings", android.content.Context.MODE_PRIVATE)
    }
    var refreshInterval by remember(context) {
        mutableStateOf(
            TelemetryRefreshInterval.fromSeconds(
                telemetryPreferences.getInt("refresh_interval_seconds", TelemetryRefreshInterval.default.seconds),
            ),
        )
    }
    var selected by rememberSaveable { mutableStateOf(Destination.Home) }
    var endpoint by rememberSaveable { mutableStateOf("") }
    var pin by rememberSaveable { mutableStateOf("") }
    var code by rememberSaveable { mutableStateOf("") }
    var token by remember { mutableStateOf<String?>(null) }
    var status by remember { mutableStateOf("Disconnected") }
    var error by remember { mutableStateOf<String?>(null) }
    var loading by remember { mutableStateOf(false) }
    var snapshot by remember { mutableStateOf(RemoteSnapshot()) }
    var eventCounter by remember { mutableIntStateOf(0) }
    var discoveredServers by remember { mutableStateOf(emptyList<DiscoveredServer>()) }
    var discoveryStatus by remember { mutableStateOf<String?>(null) }
    var discovering by remember { mutableStateOf(false) }
    var scanningQr by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val capabilities = remember(snapshot.desktop) { CompositorCapabilities.fromDesktop(snapshot.desktop) }
    val destinations = remember(capabilities) { visibleDestinations(capabilities) }

    LaunchedEffect(destinations, selected) {
        if (selected !in destinations) selected = Destination.Home
    }

    LaunchedEffect(Unit) {
        store.load()?.let { (savedEndpoint, savedPin, savedToken) ->
            endpoint = savedEndpoint; pin = savedPin; token = savedToken; status = "Connecting"
        }
    }
    LaunchedEffect(token, endpoint, pin, eventCounter, selected, refreshInterval) {
        if (token == null || endpoint.isBlank() || pin.isBlank()) return@LaunchedEffect
        suspend fun refresh() {
            loading = true
            try {
                val updated = withContext(Dispatchers.IO) {
                    val client = WayControlClient(endpoint, pin, token)
                    val desktop = client.request("desktop.info")
                    val fetch = compositorFetchPlan(CompositorCapabilities.fromDesktop(desktop))
                    RemoteSnapshot(
                        desktop = desktop,
                        workspaces = if (fetch.workspaces) client.request("workspace.list").optJSONArray("items") ?: JSONArray() else JSONArray(),
                        windows = if (fetch.windows) client.request("window.list").optJSONArray("items") ?: JSONArray() else JSONArray(),
                        monitors = if (fetch.monitors) client.request("monitor.list").optJSONArray("items") ?: JSONArray() else JSONArray(),
                        audio = runCatching { client.request("audio.info").put("available", true) }
                            .getOrElse { JSONObject().put("available", false) },
                        media = runCatching { client.request("media.info").put("available", true) }
                            .getOrElse { JSONObject().put("available", false) },
                    )
                }
                snapshot = updated
                status = "Connected"; error = null
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                snapshot = snapshot.cleared()
                status = "Disconnected"
                error = e.message ?: "Connection failed"
            }
            loading = false
        }
        if (lifecycle == null) {
            while (true) { refresh(); delay(refreshInterval.delayMillis(selected)) }
        } else lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            while (true) { refresh(); delay(refreshInterval.delayMillis(selected)) }
        }
    }
    LaunchedEffect(token, endpoint, pin, capabilities.events) {
        val savedToken = token ?: return@LaunchedEffect
        if (endpoint.isBlank() || pin.isBlank() || !capabilities.events) return@LaunchedEffect
        suspend fun subscribe() {
            val parentContext = currentCoroutineContext()
            withContext(Dispatchers.IO) {
                WayControlClient(endpoint, pin, savedToken).readEvents({ !parentContext.isActive }) {
                    scope.launch { eventCounter = (eventCounter + 1) % 100_000 }
                }
            }
        }
        if (lifecycle == null) {
            while (true) {
                runCatching { subscribe() }
                delay(1000)
            }
        } else lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            while (true) {
                runCatching { subscribe() }
                delay(1000)
            }
        }
    }

    MaterialTheme(colorScheme = darkColorScheme(
        primary = Accent, onPrimary = MainText, background = Background, onBackground = MainText,
        surface = SurfaceColor, onSurface = MainText, secondary = Color(0xFFB9567B)
    )) {
        Scaffold(containerColor = Background, contentWindowInsets = WindowInsets.safeDrawing,
            bottomBar = {
                NavigationBar(containerColor = Background, windowInsets = WindowInsets.navigationBars) {
                    destinations.forEach { destination ->
                        NavigationBarItem(selected = selected == destination, onClick = { selected = destination },
                            icon = { Text(destination.glyph, style = MaterialTheme.typography.titleMedium) },
                            label = { Text(destination.label, maxLines = 1) })
                    }
                }
            }) { insets ->
            Column(Modifier.fillMaxSize().padding(insets).verticalScroll(rememberScrollState())
                .padding(horizontal = 20.dp, vertical = 16.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
                when (selected) {
                    Destination.Home -> HomeScreen(status, loading, error, snapshot.desktop, snapshot.workspaces)
                    Destination.Workspaces -> WorkspaceScreen(snapshot.workspaces, status, capabilities.switchWorkspace) { id ->
                        scope.launch { runCatching { withContext(Dispatchers.IO) { WayControlClient(endpoint, pin, token).request("workspace.switch", JSONObject().put("workspace_id", id)) } }.onFailure { error = it.message } }
                    }
                    Destination.Windows -> WindowScreen(snapshot.windows, snapshot.workspaces, status, capabilities) { operation, address, payload ->
                        scope.launch { runCatching { withContext(Dispatchers.IO) { WayControlClient(endpoint, pin, token).request(operation, payload.put("address", address)) } }.onFailure { error = it.message } }
                    }
                    Destination.System -> SystemScreen(snapshot.desktop, snapshot.monitors, status, capabilities, snapshot.audio, snapshot.media) { operation, payload ->
                        scope.launch { runCatching { withContext(Dispatchers.IO) { WayControlClient(endpoint, pin, token).request(operation, payload) } }.onFailure { error = it.message } }
                    }
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
                        onDiscover = {
                            discovering = true
                            discoveryStatus = null
                            scope.launch {
                                try {
                                    discoveredServers = LanDiscovery.discover()
                                    discoveryStatus = if (discoveredServers.isEmpty()) "No WayControl servers found on this network" else "Found ${discoveredServers.size} server(s)"
                                } catch (exception: Exception) {
                                    discoveryStatus = exception.message ?: "Network discovery failed"
                                } finally {
                                    discovering = false
                                }
                            }
                        },
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
                                    token = approvedToken; status = "Connected"; error = null
                                } catch (e: Exception) { status = "Pairing failed"; error = e.message ?: "Pairing failed" }
                                finally { loading = false }
                            }
                        }, onDisconnect = {
                            store.clear(); token = null; snapshot = snapshot.cleared(); status = "Disconnected"; endpoint = ""; pin = ""; error = null
                        }, store = store, setToken = { token = it }, setStatus = { status = it }, setError = { error = it }, setLoading = { loading = it }
                    )
                }
                if (error != null && selected != Destination.Settings) Text(error!!, color = Color(0xFFE6A6B8))
            }
        }
    }
}

@Composable private fun HomeScreen(status: String, loading: Boolean, error: String?, desktop: JSONObject, workspaces: JSONArray) {
    val compositor = desktop.optJSONObject("compositor")?.optString("name") ?: "Compositor not detected"
    Header("WayControl", compositor)
    Card {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(desktop.optString("computer_name", "Desktop"), color = MainText, style = MaterialTheme.typography.titleMedium)
                Text(if (loading) "Refreshing" else error ?: status, color = SecondaryText, style = MaterialTheme.typography.bodyMedium)
            }
            Text(status.uppercase(), color = if (status == "Connected") Color(0xFF85C99A) else SecondaryText, style = MaterialTheme.typography.labelMedium)
        }
    }
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

@Composable private fun WorkspaceScreen(items: JSONArray, status: String, canSwitch: Boolean, switch: (String) -> Unit) {
    Header("Workspaces", status)
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

@Composable private fun WindowScreen(items: JSONArray, workspaces: JSONArray, status: String, capabilities: CompositorCapabilities, action: (String, String, JSONObject) -> Unit) {
    Header("Windows", status)
    if (items.length() == 0) Empty("No open windows", status)
    var closeTarget by remember { mutableStateOf<String?>(null) }
    for (i in 0 until items.length()) {
        val item = items.optJSONObject(i) ?: continue
        val address = item.optString("address")
        var menuOpen by remember(address) { mutableStateOf(false) }
        Card {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(item.optString("title", "Untitled window"), color = MainText, fontWeight = FontWeight.Medium)
                Text("${item.optString("app_id", "Unknown app")} · workspace ${item.optString("workspace_id", "—")}", color = SecondaryText)
                if (item.optBoolean("focused")) Text("FOCUSED", color = Color(0xFFB9567B), style = MaterialTheme.typography.labelSmall)
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (capabilities.focusWindow) Button(onClick = { action("window.focus", address, JSONObject()) }) { Text("Focus") }
                    if (capabilities.toggleFloating) Button(onClick = { action("window.toggle_floating", address, JSONObject()) }) { Text(if (item.optBoolean("floating")) "Tile" else "Float") }
                    if (capabilities.toggleFullscreen) Button(onClick = { action("window.toggle_fullscreen", address, JSONObject()) }) { Text(if (item.optBoolean("fullscreen")) "Exit full screen" else "Full screen") }
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (capabilities.moveWindow && capabilities.workspaces) Button(onClick = { menuOpen = true }) { Text("Move to workspace") }
                    androidx.compose.material3.DropdownMenu(expanded = menuOpen && capabilities.moveWindow && capabilities.workspaces, onDismissRequest = { menuOpen = false }) {
                        for (w in 0 until workspaces.length()) {
                            val workspace = workspaces.optJSONObject(w) ?: continue
                            val id = workspace.optString("id")
                            androidx.compose.material3.DropdownMenuItem(
                                text = { Text(workspace.optString("name", id.toString())) },
                                onClick = { menuOpen = false; action("window.move", address, JSONObject().put("workspace_id", id)) }
                            )
                        }
                    }
                    if (capabilities.closeWindow) Button(onClick = { closeTarget = address }) { Text("Close") }
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

@Composable internal fun SystemScreen(desktop: JSONObject, monitors: JSONArray, status: String, capabilities: CompositorCapabilities = CompositorCapabilities(), audio: JSONObject = JSONObject(), media: JSONObject = JSONObject(), action: (String, JSONObject) -> Unit) {
    Header("System monitor", status)
    var powerConfirmation by remember { mutableStateOf<String?>(null) }
    var monitorConfirmation by remember { mutableStateOf<String?>(null) }
    val memory = desktop.optJSONObject("memory")
    listOf(
        "CPU utilization" to desktop.optDouble("cpu_usage_percent", Double.NaN).let { if (it.isNaN()) "Unavailable" else "%.1f%%".format(it) },
        "CPU frequency" to desktop.optLong("cpu_frequency_khz", -1).let { if (it < 0) "Unavailable" else "${it / 1000} MHz" }, "Memory used" to (memory?.let { bytes(it.optLong("used_bytes")) } ?: "Unavailable"),
        "Memory total" to (memory?.let { bytes(it.optLong("total_bytes")) } ?: "Unavailable"),
        "Swap used" to (memory?.let { bytes(it.optLong("swap_used_bytes")) } ?: "Unavailable"),
        "NVIDIA GPU" to gpuSummary(desktop), "GPU temperature" to gpuTemperature(desktop),
        "GPU VRAM" to gpuMemory(desktop), "GPU power" to gpuPower(desktop),
        "Network throughput" to networkThroughput(desktop), "Load average" to loadAverageSummary(desktop),
        "Uptime" to desktop.optDouble("uptime_seconds", Double.NaN).let { if (it.isNaN()) "Unavailable" else duration(it) }
    ).forEach { (title, value) ->
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
                val title = player.optString("title")
                val artist = player.optString("artist")
                val album = player.optString("album")
                if (title.isNotBlank()) Text(title, color = MainText, fontWeight = FontWeight.Medium)
                val details = listOf(artist, album).filter { it.isNotBlank() }.joinToString(" · ")
                if (details.isNotBlank()) Text(details, color = SecondaryText)
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(onClick = { action("media.previous", JSONObject().put("player_id", playerId)) }) { Text("Previous") }
                    Button(onClick = { action("media.play_pause_for", JSONObject().put("player_id", playerId)) }) { Text("Play / pause") }
                    Button(onClick = { action("media.next", JSONObject().put("player_id", playerId)) }) { Text("Next") }
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

@Composable private fun SettingsScreen(endpoint: String, setEndpoint: (String) -> Unit, pin: String, setPin: (String) -> Unit,
    code: String, setCode: (String) -> Unit, status: String, error: String?, loading: Boolean, onPair: () -> Unit,
    refreshInterval: TelemetryRefreshInterval, onRefreshIntervalChange: (TelemetryRefreshInterval) -> Unit,
    discoveredServers: List<DiscoveredServer>, discoveryStatus: String?, discovering: Boolean, onDiscover: () -> Unit,
    scanningQr: Boolean, onScanQr: () -> Unit,
    onDisconnect: () -> Unit, store: SecureConnectionStore, setToken: (String) -> Unit,
    setStatus: (String) -> Unit, setError: (String?) -> Unit, setLoading: (Boolean) -> Unit) {
    Header("Settings", "Secure desktop connection")
    TelemetryRefreshSection(refreshInterval, onRefreshIntervalChange)
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
