package dev.waycontrol

import java.io.ByteArrayInputStream
import java.io.IOException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class BoundedHttpResponseTest {
    @Test
    fun acceptsResponseExactlyAtTheConfiguredLimit() {
        assertEquals("four", readBoundedResponse(ByteArrayInputStream("four".toByteArray()), 4))
    }

    @Test
    fun rejectsResponseOneByteOverTheConfiguredLimit() {
        assertThrows(IOException::class.java) {
            readBoundedResponse(ByteArrayInputStream("five!".toByteArray()), 4)
        }
    }
}
