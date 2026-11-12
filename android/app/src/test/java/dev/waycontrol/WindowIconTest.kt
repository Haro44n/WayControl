package dev.waycontrol

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertNull
import org.junit.Test

class WindowIconTest {
    @Test
    fun acceptsOnlyBoundedPngWindowIcons() {
        val png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLttAAAAABJRU5ErkJggg=="
        val decoded = windowIconBytes(png)
        assertArrayEquals(java.util.Base64.getDecoder().decode(png), decoded)
        assertNull(windowIconBytes("not-an-image"))
        assertNull(windowIconBytes("A".repeat(50_000)))
    }
}
