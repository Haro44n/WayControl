package dev.waycontrol

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import org.json.JSONObject
import java.net.URL
import java.net.URI
import java.security.KeyStore
import java.security.MessageDigest
import java.security.cert.X509Certificate
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import javax.net.ssl.HttpsURLConnection
import javax.net.ssl.SSLContext
import javax.net.ssl.TrustManager
import javax.net.ssl.X509TrustManager

object CertificatePin {
    fun canonicalize(value: String): String {
        val pin = value.replace(":", "").trim().lowercase()
        require(pin.matches(Regex("[0-9a-f]{64}"))) { "Certificate SHA-256 fingerprint must be 64 hex characters" }
        return pin
    }
}

object EndpointAddress {
    fun normalize(value: String): String {
        val uri = try { URI(value.trim()) } catch (error: Exception) {
            throw IllegalArgumentException("Server address is invalid", error)
        }
        require(uri.scheme.equals("https", ignoreCase = true)) { "TLS is required" }
        require(uri.isAbsolute && !uri.host.isNullOrBlank() && uri.rawUserInfo == null) { "Server address is invalid" }
        require(uri.rawPath.isNullOrEmpty() || uri.rawPath == "/") { "Enter only the HTTPS server origin" }
        require(uri.rawQuery == null && uri.rawFragment == null) { "Server address must not include a query or fragment" }
        return URI("https", null, uri.host, uri.port, null, null, null).toASCIIString().trimEnd('/')
    }
}

data class PairingStart(val requestId: String, val pollAfterMs: Long)

internal fun requestEnvelope(operation: String, requestId: String, payload: JSONObject): JSONObject =
    JSONObject().put("version", 1).put("id", requestId).put("type", operation).apply {
        if (payload.length() > 0) put("payload", payload)
    }

/** HTTPS client that accepts only the exact certificate shown by the trusted Linux CLI. */
class WayControlClient(baseUrl: String, fingerprint: String, private val token: String? = null) {
    private val base = EndpointAddress.normalize(baseUrl)
    private val pin = CertificatePin.canonicalize(fingerprint)
    private val tls: SSLContext

    init {
        require(base.startsWith("https://")) { "TLS is required" }
        URL(base).host.also { require(it.isNotBlank()) { "Server address is invalid" } }
        val trust = object : X509TrustManager {
            override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()
            override fun checkClientTrusted(chain: Array<X509Certificate>, authType: String) = Unit
            override fun checkServerTrusted(chain: Array<X509Certificate>, authType: String) {
                require(chain.isNotEmpty()) { "Server did not present a certificate" }
                val actual = MessageDigest.getInstance("SHA-256").digest(chain[0].encoded).joinToString("") { "%02x".format(it) }
                if (!MessageDigest.isEqual(actual.toByteArray(), pin.toByteArray())) {
                    throw java.security.cert.CertificateException("Server certificate fingerprint does not match")
                }
            }
        }
        tls = SSLContext.getInstance("TLS").apply { init(null, arrayOf<TrustManager>(trust), null) }
    }

    fun beginPairing(code: String, deviceName: String): PairingStart {
        val json = JSONObject().put("code", code).put("device_name", deviceName)
        val result = call("POST", "/api/v1/pair", json)
        return PairingStart(result.getString("request_id"), result.optLong("poll_after_ms", 800))
    }

    fun pairingStatus(requestId: String): JSONObject = call("GET", "/api/v1/pair/$requestId", null)

    fun readEvents(
        shouldStop: () -> Boolean,
        onConnected: () -> Unit,
        onEvent: (String) -> Unit,
    ) {
        check(token != null) { "Pairing approval is required" }
        val connection = (URL(base + "/api/v1/events").openConnection() as HttpsURLConnection).apply {
            sslSocketFactory = tls.socketFactory
            instanceFollowRedirects = false
            connectTimeout = 5000
            readTimeout = 15000
            requestMethod = "GET"
            setRequestProperty("Accept", "text/event-stream")
            setRequestProperty("Authorization", "Bearer $token")
        }
        try {
            if (connection.responseCode != 200) error("Event subscription failed: HTTP ${connection.responseCode}")
            onConnected()
            connection.inputStream.bufferedReader().use { reader ->
                val data = StringBuilder()
                while (!shouldStop()) {
                    val line = reader.readLine() ?: return
                    if (line.startsWith("data:")) {
                        if (data.isNotEmpty()) data.append('\n')
                        data.append(line.removePrefix("data:").trimStart())
                    } else if (line.isEmpty() && data.isNotEmpty()) {
                        onEvent(data.toString())
                        data.clear()
                    }
                }
            }
        } finally { connection.disconnect() }
    }

    fun request(operation: String, payload: JSONObject = JSONObject()): JSONObject {
        check(token != null) { "Pairing approval is required" }
        val body = requestEnvelope(operation, "android-${System.nanoTime()}", payload)
        val result = call("POST", "/api/v1/request", body, token)
        check(result.optBoolean("success")) { result.optJSONObject("error")?.optString("message") ?: "Request failed" }
        val responsePayload = result.opt("payload")
        return when (responsePayload) {
            is JSONObject -> responsePayload
            is org.json.JSONArray -> JSONObject().put("items", responsePayload)
            else -> JSONObject()
        }
    }

    private fun call(method: String, path: String, body: JSONObject?, auth: String? = null): JSONObject {
        val connection = (URL(base + path).openConnection() as HttpsURLConnection).apply {
            sslSocketFactory = tls.socketFactory
            instanceFollowRedirects = false
            connectTimeout = 5000
            readTimeout = 5000
            requestMethod = method
            setRequestProperty("Accept", "application/json")
            if (auth != null) setRequestProperty("Authorization", "Bearer $auth")
            if (body != null) {
                doOutput = true
                setRequestProperty("Content-Type", "application/json")
            }
        }
        try {
            if (body != null) connection.outputStream.use { it.write(body.toString().toByteArray(Charsets.UTF_8)) }
            val stream = if (connection.responseCode in 200..299) connection.inputStream else connection.errorStream
            val response = stream?.bufferedReader()?.use { it.readText() } ?: "{}"
            if (connection.responseCode !in 200..299) {
                val message = runCatching { JSONObject(response).getJSONObject("error").getString("message") }.getOrDefault("HTTP ${connection.responseCode}")
                error(message)
            }
            return JSONObject(response)
        } finally { connection.disconnect() }
    }
}

/** AES-GCM encrypted credential store backed by a non-exportable Android Keystore key. */
class SecureConnectionStore(context: Context) {
    private val prefs = context.getSharedPreferences("trusted_connection", Context.MODE_PRIVATE)
    private val keyAlias = "waycontrol.connection.v1"

    fun save(endpoint: String, fingerprint: String, token: String) {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding").apply { init(Cipher.ENCRYPT_MODE, key()) }
        val encrypted = cipher.doFinal(token.toByteArray(Charsets.UTF_8))
        prefs.edit().putString("endpoint", endpoint).putString("fingerprint", CertificatePin.canonicalize(fingerprint))
            .putString("token", android.util.Base64.encodeToString(cipher.iv + encrypted, android.util.Base64.NO_WRAP)).apply()
    }

    fun load(): Triple<String, String, String>? = try {
        val endpoint = prefs.getString("endpoint", null) ?: return null
        val pin = prefs.getString("fingerprint", null) ?: return null
        val data = android.util.Base64.decode(prefs.getString("token", null), android.util.Base64.NO_WRAP)
        val iv = data.copyOfRange(0, 12)
        val encrypted = data.copyOfRange(12, data.size)
        val token = Cipher.getInstance("AES/GCM/NoPadding").run {
            init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, iv))
            doFinal(encrypted).toString(Charsets.UTF_8)
        }
        Triple(endpoint, pin, token)
    } catch (_: Exception) { null }

    fun clear() { prefs.edit().clear().apply() }

    private fun key(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getKey(keyAlias, null) as? SecretKey)?.let { return it }
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").run {
            init(KeyGenParameterSpec.Builder(keyAlias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM).setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE).build())
            generateKey()
        }
    }
}
