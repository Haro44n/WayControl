package dev.waycontrol

import java.io.IOException
import java.io.PushbackReader
import java.io.StringReader
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Test

class BoundedSseTest {
    @Test
    fun readsLineAtTheCharacterLimitAndHandlesCrLf() {
        val reader = PushbackReader(StringReader("four\r\nnext\n"), 1)
        assertEquals("four", readBoundedSseLine(reader, 4))
        assertEquals("next", readBoundedSseLine(reader, 4))
        assertNull(readBoundedSseLine(reader, 4))
    }

    @Test
    fun rejectsLineOneCharacterOverTheLimit() {
        assertThrows(IOException::class.java) {
            readBoundedSseLine(PushbackReader(StringReader("five!\n"), 1), 4)
        }
    }

    @Test
    fun permitsMultilineEventExactlyAtTheLimit() {
        val data = StringBuilder()
        appendBoundedSseDataLine(data, "ab", 5)
        appendBoundedSseDataLine(data, "cd", 5)
        assertEquals("ab\ncd", data.toString())
    }

    @Test
    fun rejectsMultilineEventThatExceedsTheLimit() {
        val data = StringBuilder()
        appendBoundedSseDataLine(data, "ab", 4)
        assertThrows(IOException::class.java) {
            appendBoundedSseDataLine(data, "cd", 4)
        }
        assertEquals("ab", data.toString())
    }
}
