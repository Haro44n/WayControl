package dev.waycontrol

import android.graphics.Bitmap
import android.graphics.BitmapFactory

private const val MAX_WINDOW_ICON_BYTES = 32 * 1024
private const val MAX_WINDOW_ICON_BASE64_CHARS = 44_000
private const val MAX_WINDOW_ICON_DIMENSION = 512
private val PNG_SIGNATURE = byteArrayOf(0x89.toByte(), 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a)

internal fun windowIconBytes(encoded: String): ByteArray? {
    if (encoded.isBlank() || encoded.length > MAX_WINDOW_ICON_BASE64_CHARS) return null
    val bytes = runCatching { java.util.Base64.getDecoder().decode(encoded) }.getOrNull() ?: return null
    if (bytes.size !in 24..MAX_WINDOW_ICON_BYTES || !bytes.copyOfRange(0, 8).contentEquals(PNG_SIGNATURE)) return null
    if (!bytes.copyOfRange(12, 16).contentEquals(byteArrayOf(0x49, 0x48, 0x44, 0x52))) return null
    val width = readPngDimension(bytes, 16)
    val height = readPngDimension(bytes, 20)
    if (width !in 1..MAX_WINDOW_ICON_DIMENSION || height !in 1..MAX_WINDOW_ICON_DIMENSION) return null
    return bytes
}

internal fun decodeWindowIcon(encoded: String): Bitmap? {
    val bytes = windowIconBytes(encoded) ?: return null
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
    if (bounds.outWidth !in 1..MAX_WINDOW_ICON_DIMENSION || bounds.outHeight !in 1..MAX_WINDOW_ICON_DIMENSION) return null
    return BitmapFactory.decodeByteArray(bytes, 0, bytes.size, BitmapFactory.Options().apply {
        inPreferredConfig = Bitmap.Config.ARGB_8888
    })
}

private fun readPngDimension(bytes: ByteArray, offset: Int): Int =
    ((bytes[offset].toInt() and 0xff) shl 24) or
        ((bytes[offset + 1].toInt() and 0xff) shl 16) or
        ((bytes[offset + 2].toInt() and 0xff) shl 8) or
        (bytes[offset + 3].toInt() and 0xff)
