package dev.waycontrol

import java.io.IOException
import java.io.PushbackReader

internal const val MAX_SSE_LINE_CHARS = 16 * 1024
internal const val MAX_SSE_EVENT_CHARS = 16 * 1024

internal fun readBoundedSseLine(reader: PushbackReader, maxChars: Int): String? {
    require(maxChars >= 0) { "SSE line limit must not be negative" }
    val line = StringBuilder(minOf(maxChars, 128))
    while (true) {
        val value = reader.read()
        if (value < 0) return line.toString().takeIf { it.isNotEmpty() }
        when (value) {
            '\n'.code -> return line.toString()
            '\r'.code -> {
                val next = reader.read()
                if (next >= 0 && next != '\n'.code) reader.unread(next)
                return line.toString()
            }
            else -> {
                if (line.length >= maxChars) {
                    throw IOException("SSE line exceeds the $maxChars-character limit")
                }
                line.append(value.toChar())
            }
        }
    }
}

internal fun appendBoundedSseDataLine(data: StringBuilder, line: String, maxChars: Int) {
    require(maxChars >= 0) { "SSE event limit must not be negative" }
    val separatorLength = if (data.isEmpty()) 0 else 1
    if (line.length > maxChars - data.length - separatorLength) {
        throw IOException("SSE event exceeds the $maxChars-character limit")
    }
    if (separatorLength != 0) data.append('\n')
    data.append(line)
}
