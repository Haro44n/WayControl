package dev.waycontrol

import java.net.URI
import org.json.JSONObject

internal data class PairingQrDetails(
    val endpoint: String,
    val fingerprint: String,
    val code: String,
    val expiresAtUnix: Long,
)

internal object PairingQrPayload {
    private const val MAX_PAYLOAD_CHARS = 2048
    private const val MAX_CODE_LIFETIME_SECONDS = 180

    fun decode(rawValue: String, nowEpochSeconds: Long = System.currentTimeMillis() / 1000): PairingQrDetails {
        return try {
            decodeValidated(rawValue, nowEpochSeconds)
        } catch (exception: IllegalArgumentException) {
            throw exception
        } catch (exception: Exception) {
            throw IllegalArgumentException("Pairing QR data is malformed", exception)
        }
    }

    private fun decodeValidated(rawValue: String, nowEpochSeconds: Long): PairingQrDetails {
        require(rawValue.length in 1..MAX_PAYLOAD_CHARS) { "Pairing QR is empty or too large" }
        val value = JSONObject(rawValue)
        require(value.optInt("version") == 1) { "Pairing QR version is not supported" }

        val rawEndpoint = value.getString("endpoint")
        val uri = URI(rawEndpoint)
        require(uri.scheme.equals("https", ignoreCase = true)) { "Pairing QR must use HTTPS" }
        require(uri.rawUserInfo == null && uri.rawQuery == null && uri.rawFragment == null) {
            "Pairing QR address is invalid"
        }
        require(uri.rawPath.isNullOrEmpty() || uri.rawPath == "/") { "Pairing QR must contain only a server address" }
        require(uri.port in 1..65535 && isPrivateIpv4(uri.host.orEmpty())) {
            "Pairing QR must point to a private IPv4 LAN address"
        }
        val endpoint = EndpointAddress.normalize(rawEndpoint)

        val fingerprint = value.getString("fingerprint")
        require(fingerprint.matches(Regex("(?i)[0-9a-f]{64}"))) {
            "Pairing QR certificate fingerprint is invalid"
        }
        val code = value.getString("code")
        require(code.matches(Regex("[0-9]{6}"))) { "Pairing QR code is invalid" }

        val expiresAt = value.optLong("expires_at_unix", -1)
        val remaining = expiresAt - nowEpochSeconds
        require(remaining in 1..MAX_CODE_LIFETIME_SECONDS) { "Pairing QR has expired; scan a fresh code" }

        return PairingQrDetails(endpoint, CertificatePin.canonicalize(fingerprint), code, expiresAt)
    }

    private fun isPrivateIpv4(host: String): Boolean {
        val octets = host.split('.')
        if (octets.size != 4) return false
        val values = octets.map { part ->
            if (part.isEmpty() || part.length > 3 || (part.length > 1 && part.startsWith('0')) || !part.all(Char::isDigit)) {
                return false
            }
            part.toIntOrNull()?.takeIf { it in 0..255 } ?: return false
        }
        return values[0] == 10 ||
            (values[0] == 172 && values[1] in 16..31) ||
            (values[0] == 192 && values[1] == 168) ||
            (values[0] == 169 && values[1] == 254)
    }
}
