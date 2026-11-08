package dev.waycontrol

import org.json.JSONArray
import java.util.Locale

internal data class StorageRow(
    val mountPoint: String,
    val sourceAndType: String,
    val usage: String,
)

internal fun storageRows(samples: JSONArray): List<StorageRow> = buildList {
    for (index in 0 until samples.length()) {
        val sample = samples.optJSONObject(index) ?: continue
        val mountPoint = sample.optString("mount_point").takeIf { it.isNotBlank() } ?: continue
        val source = sample.optString("source").takeIf { it.isNotBlank() } ?: "Unknown device"
        val filesystem = sample.optString("filesystem_type").takeIf { it.isNotBlank() } ?: "unknown"
        val total = sample.optLong("total_bytes", -1)
        val used = sample.optLong("used_bytes", -1)
        val available = sample.optLong("available_bytes", -1)
        if (total <= 0 || used < 0 || used > total || available < 0 || available > total) continue
        val percent = used.toDouble() / total * 100.0
        add(
            StorageRow(
                mountPoint = mountPoint,
                sourceAndType = "$source · $filesystem",
                usage = String.format(
                    Locale.ROOT,
                    "%.1f%% used · %s / %s · %s free",
                    percent,
                    bytes(used),
                    bytes(total),
                    bytes(available),
                ),
            ),
        )
    }
}
