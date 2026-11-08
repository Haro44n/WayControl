package dev.waycontrol

import java.util.Locale
import org.json.JSONArray

internal data class CpuCoreTemperatureRow(val label: String, val temperature: String)

internal fun cpuCoreTemperatureRows(samples: JSONArray): List<CpuCoreTemperatureRow> =
    (0 until samples.length()).mapNotNull { index ->
        val sample = samples.optJSONObject(index) ?: return@mapNotNull null
        val coreId = sample.optLong("core_id", -1)
            .takeIf { it in 0..Int.MAX_VALUE.toLong() } ?: return@mapNotNull null
        val packageId = if (sample.has("package_id") && !sample.isNull("package_id")) {
            sample.optLong("package_id", -1)
                .takeIf { it in 0..Int.MAX_VALUE.toLong() } ?: return@mapNotNull null
        } else {
            null
        }
        val millidegrees = sample.optLong("temperature_millidegrees", Long.MIN_VALUE)
            .takeIf { it in -100_000..250_000 } ?: return@mapNotNull null
        val label = packageId?.let { "Package $it · Core $coreId" } ?: "Core $coreId"
        CpuCoreTemperatureRow(
            label = label,
            temperature = "${"%.1f".format(Locale.ROOT, millidegrees / 1000.0)} °C",
        )
    }
