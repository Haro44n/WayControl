package dev.waycontrol

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class TelemetryHistoryTest {
    @Test
    fun appendingKeepsOnlyTheNewestSixtySamples() {
        val history = (0..60).fold(TelemetryHistory()) { current, value ->
            current.append(TelemetrySample(cpuPercent = value.toDouble(), memoryPercent = null))
        }

        assertEquals(60, history.samples.size)
        assertEquals(1.0, history.samples.first().cpuPercent!!, 0.0)
        assertEquals(60.0, history.samples.last().cpuPercent!!, 0.0)
    }

    @Test
    fun sampleExtractionUsesMemoryRatioAndOmitsUnavailableOrInvalidMetrics() {
        val sample = telemetrySample(
            JSONObject()
                .put("cpu_usage_percent", 125.0)
                .put("memory", JSONObject().put("used_bytes", 3).put("total_bytes", 4)),
        )

        assertEquals(null, sample.cpuPercent)
        assertEquals(75.0, sample.memoryPercent!!, 0.0)
        assertEquals(75.0, TelemetryHistory().append(sample).samples.single().memoryPercent!!, 0.0)
        assertTrue(telemetrySample(JSONObject()).let { it.cpuPercent == null && it.memoryPercent == null })
        assertEquals(TelemetryHistory(), TelemetryHistory().append(telemetrySample(JSONObject())))
    }

    @Test
    fun sampleExtractionCapturesGpuNetworkAndTemperatureHistory() {
        val sample = telemetrySample(
            JSONObject()
                .put("gpu", JSONObject().put("available", true).put("utilization_percent", 63).put("temperature_celsius", 71))
                .put(
                    "network_rates",
                    org.json.JSONArray()
                        .put(JSONObject().put("received_bytes_per_second", 1200).put("transmitted_bytes_per_second", 800))
                        .put(JSONObject().put("received_bytes_per_second", 300).put("transmitted_bytes_per_second", 200)),
                ),
        )

        assertEquals(63.0, sample.gpuPercent!!, 0.0)
        assertEquals(71.0, sample.gpuTemperatureCelsius!!, 0.0)
        assertEquals(1500.0, sample.networkReceivedBytesPerSecond!!, 0.0)
        assertEquals(1000.0, sample.networkTransmittedBytesPerSecond!!, 0.0)
    }
}
