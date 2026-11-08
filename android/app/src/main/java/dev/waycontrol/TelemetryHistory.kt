package dev.waycontrol

import org.json.JSONObject

internal data class TelemetrySample(
    val cpuPercent: Double?,
    val memoryPercent: Double?,
)

internal data class TelemetryHistory(
    val samples: List<TelemetrySample> = emptyList(),
    val capacity: Int = DEFAULT_CAPACITY,
) {
    init {
        require(capacity > 0)
    }

    fun append(sample: TelemetrySample): TelemetryHistory {
        val validSample = TelemetrySample(
            cpuPercent = sample.cpuPercent.takeIf(::isPercentage),
            memoryPercent = sample.memoryPercent.takeIf(::isPercentage),
        )
        if (validSample.cpuPercent == null && validSample.memoryPercent == null) return this
        return copy(samples = (samples + validSample).takeLast(capacity))
    }

    companion object {
        const val DEFAULT_CAPACITY = 60
    }
}

internal fun telemetrySample(desktop: JSONObject): TelemetrySample {
    val cpuUsage = desktop.optDouble("cpu_usage_percent", Double.NaN)
    val memory = desktop.optJSONObject("memory")
    val memoryUsed = memory?.optLong("used_bytes", -1) ?: -1
    val memoryTotal = memory?.optLong("total_bytes", -1) ?: -1
    val memoryUsage = if (memoryTotal > 0 && memoryUsed in 0..memoryTotal) {
        memoryUsed.toDouble() / memoryTotal * 100.0
    } else {
        null
    }
    return TelemetrySample(
        cpuPercent = cpuUsage.takeIf(::isPercentage),
        memoryPercent = memoryUsage?.takeIf(::isPercentage),
    )
}

private fun isPercentage(value: Double?): Boolean = value != null && value.isFinite() && value in 0.0..100.0
