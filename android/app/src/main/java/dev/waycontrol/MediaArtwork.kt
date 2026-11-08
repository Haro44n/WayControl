package dev.waycontrol

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import org.json.JSONArray
import org.json.JSONObject

internal data class MediaArtworkImage(val artworkId: String, val bitmap: Bitmap)

internal fun mediaArtworkTarget(media: JSONObject): Pair<String, String>? {
    val players = media.optJSONArray("players") ?: JSONArray()
    var fallback: Pair<String, String>? = null
    for (index in 0 until players.length()) {
        val player = players.optJSONObject(index) ?: continue
        val id = player.optString("artwork_id").takeIf { it.matches(Regex("[0-9a-f]{64}")) } ?: continue
        val playerId = player.optString("player_id").takeIf(String::isNotBlank) ?: continue
        val target = playerId to id
        if (player.optString("playback_status").equals("Playing", ignoreCase = true)) return target
        if (fallback == null) fallback = target
    }
    return fallback
}

internal fun decodeMediaArtwork(response: JSONObject): MediaArtworkImage? {
    val (id, bytes) = mediaArtworkBytes(response) ?: return null
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
    if (bounds.outWidth !in 1..4096 || bounds.outHeight !in 1..4096) return null
    var sample = 1
    while (bounds.outWidth / sample > 512 || bounds.outHeight / sample > 512) sample *= 2
    val bitmap = BitmapFactory.decodeByteArray(bytes, 0, bytes.size, BitmapFactory.Options().apply {
        inSampleSize = sample
        inPreferredConfig = Bitmap.Config.RGB_565
    }) ?: return null
    return MediaArtworkImage(id, bitmap)
}

internal fun mediaArtworkBytes(response: JSONObject): Pair<String, ByteArray>? {
    val id = response.optString("artwork_id")
    if (!id.matches(Regex("[0-9a-f]{64}"))) return null
    val encoded = response.optString("data_base64")
    if (encoded.isBlank() || encoded.length > 700_000) return null
    val bytes = runCatching { java.util.Base64.getDecoder().decode(encoded) }.getOrNull() ?: return null
    if (bytes.isEmpty() || bytes.size > 512 * 1024) return null
    return id to bytes
}
