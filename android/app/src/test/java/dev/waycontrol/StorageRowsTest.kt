package dev.waycontrol

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Test

class StorageRowsTest {
    @Test
    fun storageRowsFormatMountUsageAndSkipInvalidSamples() {
        val samples = JSONArray()
            .put(
                JSONObject()
                    .put("mount_point", "/home")
                    .put("source", "/dev/nvme0n1p2")
                    .put("filesystem_type", "btrfs")
                    .put("total_bytes", 1000)
                    .put("used_bytes", 725)
                    .put("available_bytes", 250),
            )
            .put(JSONObject().put("mount_point", "/broken").put("total_bytes", 0))

        val rows = storageRows(samples)

        assertEquals(1, rows.size)
        assertEquals("/home", rows.single().mountPoint)
        assertEquals("/dev/nvme0n1p2 · btrfs", rows.single().sourceAndType)
        assertEquals("72.5% used · 725.0 B / 1000.0 B · 250.0 B free", rows.single().usage)
    }
}
