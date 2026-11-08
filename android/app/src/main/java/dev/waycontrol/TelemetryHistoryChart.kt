package dev.waycontrol

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Card
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.text.font.FontWeight
import java.util.Locale
import kotlin.math.ceil

private val HistoryText = Color(0xFFF3F3F5)
private val HistorySecondary = Color(0xFF9999A4)
private val HistoryAccent = Color(0xFF8C3655)

@Composable
internal fun TelemetryHistoryCharts(history: TelemetryHistory) {
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        MetricHistoryChart("CPU usage", history.samples.map { it.cpuPercent }, "percent")
        MetricHistoryChart("GPU usage", history.samples.map { it.gpuPercent }, "percent")
        MetricHistoryChart("Memory usage", history.samples.map { it.memoryPercent }, "percent")
        MetricHistoryChart("GPU temperature", history.samples.map { it.gpuTemperatureCelsius }, "temperature")
        MetricHistoryChart("Network received", history.samples.map { it.networkReceivedBytesPerSecond }, "rate")
        MetricHistoryChart("Network transmitted", history.samples.map { it.networkTransmittedBytesPerSecond }, "rate")
    }
}

@Composable
private fun MetricHistoryChart(title: String, values: List<Double?>, unit: String) {
    val validValues = values.filterNotNull()
    val latest = validValues.lastOrNull()
    val description = if (latest == null) {
        "$title history, waiting for samples"
    } else {
        "$title history, ${validValues.size} samples, latest ${formatHistoryValue(latest, unit)}"
    }
    Card {
        Column(Modifier.fillMaxWidth().padding(16.dp)) {
            Text(title, color = HistoryText, fontWeight = FontWeight.Medium)
            Box(
                Modifier.fillMaxWidth().height(112.dp).semantics { contentDescription = description },
                contentAlignment = Alignment.Center,
            ) {
                if (latest == null) {
                    Text("Waiting for samples", color = HistorySecondary)
                } else {
                    Canvas(Modifier.fillMaxWidth().height(96.dp)) {
                        val graphHeight = size.height
                        val maximum = when (unit) {
                            "percent" -> 100.0
                            "temperature" -> maxOf(100.0, ceil((validValues.maxOrNull() ?: 0.0) / 10.0) * 10.0)
                            else -> maxOf(1.0, validValues.maxOrNull() ?: 0.0)
                        }
                        val horizontalSteps = 4
                        for (step in 0..horizontalSteps) {
                            val y = graphHeight * step / horizontalSteps
                            drawLine(
                                color = HistorySecondary.copy(alpha = 0.22f),
                                start = Offset(0f, y),
                                end = Offset(size.width, y),
                                strokeWidth = 1.dp.toPx(),
                            )
                        }
                        val path = Path()
                        var segmentStarted = false
                        values.forEachIndexed { index, value ->
                            if (value == null) {
                                segmentStarted = false
                            } else {
                                val x = if (values.size == 1) size.width / 2f
                                else size.width * index / (values.size - 1)
                                val y = graphHeight * (1f - (value / maximum).toFloat().coerceIn(0f, 1f))
                                if (segmentStarted) path.lineTo(x, y) else path.moveTo(x, y)
                                segmentStarted = true
                            }
                        }
                        drawPath(path, color = HistoryAccent, style = Stroke(width = 3.dp.toPx()))
                        values.forEachIndexed { index, value ->
                            if (value == null) return@forEachIndexed
                            val x = if (values.size == 1) size.width / 2f
                            else size.width * index / (values.size - 1)
                            val y = graphHeight * (1f - (value / maximum).toFloat().coerceIn(0f, 1f))
                            drawCircle(color = HistoryAccent, radius = 2.5.dp.toPx(), center = Offset(x, y))
                        }
                    }
                }
            }
            Text(
                latest?.let { "${formatHistoryValue(it, unit)} · ${validValues.size} recent samples" } ?: "Waiting for samples",
                color = HistorySecondary,
                style = androidx.compose.material3.MaterialTheme.typography.bodySmall,
            )
        }
    }
}

private fun formatHistoryValue(value: Double, unit: String): String = when (unit) {
    "percent" -> "${"%.1f".format(Locale.ROOT, value)}%"
    "temperature" -> "${"%.1f".format(Locale.ROOT, value)} °C"
    else -> "${formatHistoryRate(value)}"
}

private fun formatHistoryRate(bytesPerSecond: Double): String = when {
    bytesPerSecond < 1024 -> "${"%.0f".format(Locale.ROOT, bytesPerSecond)} B/s"
    bytesPerSecond < 1024 * 1024 -> "${"%.1f".format(Locale.ROOT, bytesPerSecond / 1024)} KiB/s"
    else -> "${"%.1f".format(Locale.ROOT, bytesPerSecond / (1024 * 1024))} MiB/s"
}
