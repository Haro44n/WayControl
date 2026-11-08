package dev.waycontrol

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Test

class CpuCoreTemperatureRowsTest {
    @Test
    fun formatsPerPackageCoreTemperatureRows() {
        val samples = JSONArray().put(
            JSONObject()
                .put("package_id", 1)
                .put("core_id", 12)
                .put("temperature_millidegrees", 42_875),
        )

        assertEquals(
            listOf(CpuCoreTemperatureRow("Package 1 · Core 12", "42.9 °C")),
            cpuCoreTemperatureRows(samples),
        )
    }

    @Test
    fun skipsMissingOrInvalidTemperatureSamples() {
        val samples = JSONArray()
            .put(JSONObject().put("core_id", 0).put("temperature_millidegrees", 34_000))
            .put(JSONObject().put("core_id", -1).put("temperature_millidegrees", 34_000))
            .put(JSONObject().put("core_id", 2).put("temperature_millidegrees", 300_000))

        assertEquals(
            listOf(CpuCoreTemperatureRow("Core 0", "34.0 °C")),
            cpuCoreTemperatureRows(samples),
        )
    }
}
