package dev.waycontrol

import org.json.JSONObject

internal data class TelemetrySample(
    val cpuPercent: Double?,
    val memoryPercent: Double?,
    val gpuPercent: Double? = null,
    val gpuTemperatureCelsius: Double? = null,
    val networkReceivedBytesPerSecond: Double? = null,
    val networkTransmittedBytesPerSecond: Double? = null,
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
            gpuPercent = sample.gpuPercent.takeIf(::isPercentage),
            gpuTemperatureCelsius = sample.gpuTemperatureCelsius.takeIf { it != null && it.isFinite() && it in 0.0..150.0 },
            networkReceivedBytesPerSecond = sample.networkReceivedBytesPerSecond.takeIf(::isRate),
            networkTransmittedBytesPerSecond = sample.networkTransmittedBytesPerSecond.takeIf(::isRate),
        )
        if (validSample.hasNoMetrics()) return this
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
    val gpu = desktop.optJSONObject("gpu")
    val gpuPercent = if (gpu?.optBoolean("available") == true) {
        gpu.optDouble("utilization_percent", Double.NaN).takeIf(::isPercentage)
    } else null
    val gpuTemperature = if (gpu?.optBoolean("available") == true) {
        gpu.optDouble("temperature_celsius", Double.NaN).takeIf { it.isFinite() && it in 0.0..150.0 }
    } else null
    val rates = desktop.optJSONArray("network_rates")
    var received = 0.0
    var transmitted = 0.0
    var hasReceived = false
    var hasTransmitted = false
    if (rates != null) for (index in 0 until rates.length()) {
        val rate = rates.optJSONObject(index) ?: continue
        rate.optDouble("received_bytes_per_second", Double.NaN).takeIf(::isRate)?.let {
            received += it
            hasReceived = true
        }
        rate.optDouble("transmitted_bytes_per_second", Double.NaN).takeIf(::isRate)?.let {
            transmitted += it
            hasTransmitted = true
        }
    }
    return TelemetrySample(
        cpuPercent = cpuUsage.takeIf(::isPercentage),
        memoryPercent = memoryUsage?.takeIf(::isPercentage),
        gpuPercent = gpuPercent,
        gpuTemperatureCelsius = gpuTemperature,
        networkReceivedBytesPerSecond = received.takeIf { hasReceived && isRate(it) },
        networkTransmittedBytesPerSecond = transmitted.takeIf { hasTransmitted && isRate(it) },
    )
}

private fun isPercentage(value: Double?): Boolean = value != null && value.isFinite() && value in 0.0..100.0

private fun isRate(value: Double?): Boolean = value != null && value.isFinite() && value >= 0.0

private fun TelemetrySample.hasNoMetrics(): Boolean = cpuPercent == null && memoryPercent == null && gpuPercent == null &&
    gpuTemperatureCelsius == null && networkReceivedBytesPerSecond == null && networkTransmittedBytesPerSecond == null
