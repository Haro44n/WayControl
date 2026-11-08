package dev.waycontrol

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Test

class GpuOptionalMetricsTest {
    @Test
    fun gpuOptionalMetricsFormatSupportedValuesAndKeepMissingValuesUnavailable() {
        val rows = gpuOptionalMetricRows(
            JSONObject()
                .put("fan_speed_percent", 37)
                .put("graphics_clock_mhz", 1815)
                .put("power_limit_watts", 250.0),
        )

        assertEquals(
            listOf(
                GpuOptionalMetricRow("GPU fan", "37%"),
                GpuOptionalMetricRow("GPU clock", "1815 MHz"),
                GpuOptionalMetricRow("GPU power limit", "250.0 W"),
            ),
            rows,
        )
    }

    @Test
    fun gpuOptionalMetricsRejectOutOfRangeValues() {
        val rows = gpuOptionalMetricRows(
            JSONObject()
                .put("fan_speed_percent", 120)
                .put("graphics_clock_mhz", 0)
                .put("power_limit_watts", -1.0),
        )

        assertEquals(listOf("Unavailable", "Unavailable", "Unavailable"), rows.map { it.value })
    }
}
