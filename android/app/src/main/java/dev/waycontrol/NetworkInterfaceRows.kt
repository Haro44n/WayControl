package dev.waycontrol

import org.json.JSONArray
import java.util.Locale

internal data class NetworkInterfaceRow(
    val name: String,
    val linkState: String,
    val addresses: String,
    val receivedBytesPerSecond: Double?,
    val transmittedBytesPerSecond: Double?,
)

internal fun networkInterfaceRows(interfaces: JSONArray, rates: JSONArray): List<NetworkInterfaceRow> {
    val ratesByName = buildMap {
        for (index in 0 until rates.length()) {
            val sample = rates.optJSONObject(index) ?: continue
            val name = sample.optString("name").takeIf { it.isNotBlank() } ?: continue
            val received = sample.optDouble("received_bytes_per_second", Double.NaN)
                .takeIf { it.isFinite() && it >= 0 }
            val transmitted = sample.optDouble("transmitted_bytes_per_second", Double.NaN)
                .takeIf { it.isFinite() && it >= 0 }
            put(name, received to transmitted)
        }
    }

    return buildList {
        for (index in 0 until interfaces.length()) {
            val sample = interfaces.optJSONObject(index) ?: continue
            val name = sample.optString("name").takeIf { it.isNotBlank() } ?: continue
            val isUp = sample.optBoolean("is_up")
            val state = sample.optString("operational_state")
                .takeIf { it.isNotBlank() && it != "null" }
                ?: if (isUp) "up" else "down"
            val addresses = sample.optJSONArray("addresses") ?: JSONArray()
            val addressList = (0 until addresses.length())
                .mapNotNull { addresses.optString(it).takeIf(String::isNotBlank) }
                .distinct()
            val route = sample.optBoolean("default_route")
            val linkState = state.replaceFirstChar { it.titlecase(Locale.ROOT) } +
                if (route) " · Default route" else ""
            val (received, transmitted) = ratesByName[name] ?: (null to null)
            add(
                NetworkInterfaceRow(
                    name = name,
                    linkState = linkState,
                    addresses = addressList.joinToString(" · ").ifEmpty { "No IP address" },
                    receivedBytesPerSecond = received,
                    transmittedBytesPerSecond = transmitted,
                ),
            )
        }
    }
}
