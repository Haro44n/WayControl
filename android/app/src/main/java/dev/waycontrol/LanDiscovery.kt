package dev.waycontrol

import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.Inet4Address
import java.net.InetAddress
import java.net.SocketTimeoutException
import org.json.JSONObject
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

internal data class DiscoveredServer(val name: String, val endpoint: String)

internal object LanDiscovery {
    const val PORT = 8766
    private const val PROBE = "WAYCONTROL_DISCOVER_V1"
    private const val MAX_RESPONSE_BYTES = 2048
    private const val MAX_RESULTS = 32

    fun decodeAnnouncement(payload: ByteArray, sourceAddress: InetAddress): DiscoveredServer? {
        return try {
            if (payload.isEmpty() || payload.size > MAX_RESPONSE_BYTES || sourceAddress !is Inet4Address) return null
            if (!sourceAddress.isSiteLocalAddress && !sourceAddress.isLinkLocalAddress) return null
            val response = JSONObject(payload.toString(Charsets.UTF_8))
            if (response.optInt("version") != 1) return null
            val port = response.optInt("https_port", -1)
            if (port !in 1..65535) return null
            val name = response.optString("name")
                .filterNot(Char::isISOControl)
                .take(64)
                .trim()
                .ifBlank { "WayControl server" }
            val endpoint = EndpointAddress.normalize("https://${sourceAddress.hostAddress}:$port")
            DiscoveredServer(name, endpoint)
        } catch (_: Exception) {
            null
        }
    }

    suspend fun discover(timeoutMillis: Long = 1500): List<DiscoveredServer> = withContext(Dispatchers.IO) {
        require(timeoutMillis in 200..5000) { "Discovery timeout must be between 200 ms and 5 seconds" }
        DatagramSocket().use { socket ->
            socket.broadcast = true
            socket.soTimeout = 200
            val probe = PROBE.toByteArray(Charsets.US_ASCII)
            socket.send(DatagramPacket(probe, probe.size, InetAddress.getByName("255.255.255.255"), PORT))
            val deadline = System.nanoTime() + timeoutMillis * 1_000_000
            val found = linkedMapOf<String, DiscoveredServer>()
            val buffer = ByteArray(MAX_RESPONSE_BYTES)
            while (System.nanoTime() < deadline && found.size < MAX_RESULTS) {
                val remainingMillis = ((deadline - System.nanoTime()) / 1_000_000).toInt()
                if (remainingMillis <= 0) break
                socket.soTimeout = remainingMillis.coerceAtMost(200).coerceAtLeast(1)
                val packet = DatagramPacket(buffer, buffer.size)
                try {
                    socket.receive(packet)
                    decodeAnnouncement(packet.data.copyOf(packet.length), packet.address)?.let { server ->
                        found.putIfAbsent(server.endpoint, server)
                    }
                } catch (_: SocketTimeoutException) {
                    // Keep listening until the caller's bounded discovery window closes.
                }
            }
            found.values.sortedBy { it.name.lowercase() }
        }
    }
}
