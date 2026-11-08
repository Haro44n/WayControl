package dev.waycontrol

import org.json.JSONObject
import java.util.Locale

internal data class GpuOptionalMetricRow(val label: String, val value: String)

internal fun gpuOptionalMetricRows(gpu: JSONObject?): List<GpuOptionalMetricRow> {
    val fan = gpu?.optInt("fan_speed_percent", -1)
        ?.takeIf { it in 0..100 }
        ?.let { "$it%" }
        ?: "Unavailable"
    val clock = gpu?.optInt("graphics_clock_mhz", -1)
        ?.takeIf { it > 0 }
        ?.let { "$it MHz" }
        ?: "Unavailable"
    val powerLimit = gpu?.optDouble("power_limit_watts", Double.NaN)
        ?.takeIf { it.isFinite() && it > 0 }
        ?.let { String.format(Locale.ROOT, "%.1f W", it) }
        ?: "Unavailable"
    return listOf(
        GpuOptionalMetricRow("GPU fan", fan),
        GpuOptionalMetricRow("GPU clock", clock),
        GpuOptionalMetricRow("GPU power limit", powerLimit),
    )
}
