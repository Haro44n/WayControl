package dev.waycontrol

import org.junit.Assert.assertEquals
import org.junit.Test

class CertificatePinTest {
    @Test fun acceptsSha256HexAndReturnsCanonicalPin() {
        val pin = "AA".repeat(32)
        assertEquals("aa".repeat(32), CertificatePin.canonicalize(pin))
    }

    @Test(expected = IllegalArgumentException::class)
    fun rejectsShortPins() {
        CertificatePin.canonicalize("deadbeef")
    }

    @Test fun acceptsOnlyAnHttpsOriginForTheServerAddress() {
        assertEquals("https://192.168.1.20:8765", EndpointAddress.normalize("https://192.168.1.20:8765/"))
    }

    @Test(expected = IllegalArgumentException::class)
    fun rejectsCleartextServerAddresses() {
        EndpointAddress.normalize("http://192.168.1.20:8765")
    }

    @Test(expected = IllegalArgumentException::class)
    fun rejectsServerAddressesWithPaths() {
        EndpointAddress.normalize("https://192.168.1.20:8765/api")
    }
}
