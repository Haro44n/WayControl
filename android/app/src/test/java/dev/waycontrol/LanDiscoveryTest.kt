package dev.waycontrol

import java.net.InetAddress
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class LanDiscoveryTest {
    @Test
    fun validPrivateLanAnnouncementBuildsTlsEndpointFromPacketSource() {
        val response = """{"version":1,"name":"Living room","https_port":8765}""".toByteArray()

        val server = LanDiscovery.decodeAnnouncement(response, InetAddress.getByName("192.168.1.20"))

        assertEquals(DiscoveredServer("Living room", "https://192.168.1.20:8765"), server)
    }

    @Test
    fun rejectsMalformedUnsupportedAndNonLanAnnouncements() {
        val source = InetAddress.getByName("192.168.1.20")
        assertNull(LanDiscovery.decodeAnnouncement("{}".toByteArray(), source))
        assertNull(LanDiscovery.decodeAnnouncement("x".repeat(2049).toByteArray(), source))
        assertNull(
            LanDiscovery.decodeAnnouncement(
                """{"version":2,"name":"Other","https_port":8765}""".toByteArray(),
                source,
            ),
        )
        assertNull(
            LanDiscovery.decodeAnnouncement(
                """{"version":1,"name":"Other","https_port":0}""".toByteArray(),
                source,
            ),
        )
        assertNull(
            LanDiscovery.decodeAnnouncement(
                """{"version":1,"name":"Internet","https_port":8765}""".toByteArray(),
                InetAddress.getByName("8.8.8.8"),
            ),
        )
    }

    @Test
    fun stripsControlCharactersAndBoundsDiscoveredName() {
        val response = """{"version":1,"name":"Desk\nName","https_port":8765}""".toByteArray()

        val server = LanDiscovery.decodeAnnouncement(response, InetAddress.getByName("10.1.2.3"))

        assertEquals("DeskName", server?.name)
    }
}
