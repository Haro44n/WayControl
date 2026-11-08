package dev.waycontrol

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Test

class NetworkInterfaceRowsTest {
    @Test
    fun networkInterfaceRowsCombineLinkStateAddressesAndRates() {
        val interfaces = JSONArray().put(
            JSONObject()
                .put("name", "wlan0")
                .put("is_up", true)
                .put("operational_state", "up")
                .put("default_route", true)
                .put("addresses", JSONArray().put("192.168.1.10").put("fe80::1")),
        )
        val rates = JSONArray().put(
            JSONObject()
                .put("name", "wlan0")
                .put("received_bytes_per_second", 1024.0)
                .put("transmitted_bytes_per_second", 2048.0),
        )

        val row = networkInterfaceRows(interfaces, rates).single()

        assertEquals("wlan0", row.name)
        assertEquals("Up · Default route", row.linkState)
        assertEquals("192.168.1.10 · fe80::1", row.addresses)
        assertEquals(1024.0, row.receivedBytesPerSecond!!, 0.0)
        assertEquals(2048.0, row.transmittedBytesPerSecond!!, 0.0)
    }
}
