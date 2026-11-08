package dev.waycontrol

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class PairingQrPayloadTest {
    @Test
    fun decodesPrivateLanEndpointAndPinnedPairingFields() {
        val payload = """{"version":1,"endpoint":"https://192.168.1.20:8765","fingerprint":"${"a".repeat(64)}","code":"042381","expires_at_unix":1800000120}"""

        val decoded = PairingQrPayload.decode(payload, nowEpochSeconds = 1_800_000_000)

        assertEquals("https://192.168.1.20:8765", decoded.endpoint)
        assertEquals("a".repeat(64), decoded.fingerprint)
        assertEquals("042381", decoded.code)
    }

    @Test
    fun rejectsExpiredUntrustedOrMalformedPairingCodes() {
        val fingerprint = "b".repeat(64)
        fun payload(endpoint: String, code: String, expiry: Long) =
            """{"version":1,"endpoint":"$endpoint","fingerprint":"$fingerprint","code":"$code","expires_at_unix":$expiry}"""

        assertThrows(IllegalArgumentException::class.java) {
            PairingQrPayload.decode(payload("https://192.168.1.20:8765", "123456", 100), nowEpochSeconds = 100)
        }
        assertThrows(IllegalArgumentException::class.java) {
            PairingQrPayload.decode(payload("https://8.8.8.8:8765", "123456", 200), nowEpochSeconds = 100)
        }
        assertThrows(IllegalArgumentException::class.java) {
            PairingQrPayload.decode(payload("https://192.168.1.20:8765", "12x456", 200), nowEpochSeconds = 100)
        }
        assertThrows(IllegalArgumentException::class.java) {
            PairingQrPayload.decode("not-json", nowEpochSeconds = 100)
        }
    }

    @Test
    fun rejectsExpiryTimestampThatWouldOverflowWhenSubtracted() {
        val payload = """{"version":1,"endpoint":"https://192.168.1.20:8765","fingerprint":"${"a".repeat(64)}","code":"042381","expires_at_unix":${Long.MIN_VALUE}}"""

        assertThrows(IllegalArgumentException::class.java) {
            PairingQrPayload.decode(payload, nowEpochSeconds = Long.MAX_VALUE)
        }
    }
}
