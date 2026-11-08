package dev.waycontrol

import org.json.JSONObject

internal data class MediaPlayerPresentation(
    val canGoPrevious: Boolean,
    val showPlayPause: Boolean,
    val playPauseLabel: String,
    val canGoNext: Boolean,
    val positionText: String?,
)

internal fun mediaPlayerPresentation(player: JSONObject): MediaPlayerPresentation {
    fun capability(name: String): Boolean? {
        if (!player.has(name) || player.isNull(name)) return null
        return runCatching { player.getBoolean(name) }.getOrNull()
    }

    val canPlay = capability("can_play")
    val canPause = capability("can_pause")
    val canControl = capability("can_control")
    val capabilitiesReported = player.has("can_play") || player.has("can_pause")
    val status = player.optString("playback_status")
    val showPlayPause = when {
        canControl == false -> false
        !capabilitiesReported -> true
        status.equals("Playing", ignoreCase = true) -> canPause == true
        status.equals("Paused", ignoreCase = true) || status.equals("Stopped", ignoreCase = true) -> canPlay == true
        else -> canPlay == true || canPause == true
    }
    val positionText = mediaPositionText(player)

    return MediaPlayerPresentation(
        canGoPrevious = canControl != false && supports(player, "can_go_previous"),
        showPlayPause = showPlayPause,
        playPauseLabel = when {
            !capabilitiesReported -> "Play / pause"
            status.equals("Playing", ignoreCase = true) -> "Pause"
            status.equals("Paused", ignoreCase = true) || status.equals("Stopped", ignoreCase = true) -> "Play"
            else -> "Play / pause"
        },
        canGoNext = canControl != false && supports(player, "can_go_next"),
        positionText = positionText,
    )
}

private fun supports(player: JSONObject, property: String): Boolean {
    if (!player.has(property)) return true
    return !player.isNull(property) && runCatching { player.getBoolean(property) }.getOrDefault(false)
}

private fun mediaPositionText(player: JSONObject): String? {
    if (!player.has("position_ms") || player.isNull("position_ms") ||
        !player.has("duration_ms") || player.isNull("duration_ms")
    ) return null
    val position = player.optLong("position_ms", -1)
    val duration = player.optLong("duration_ms", -1)
    if (position < 0 || duration <= 0) return null
    return "${formatMediaTime(position.coerceAtMost(duration))} / ${formatMediaTime(duration)}"
}

private fun formatMediaTime(milliseconds: Long): String {
    val totalSeconds = milliseconds / 1000
    val seconds = totalSeconds % 60
    val minutes = totalSeconds / 60 % 60
    val hours = totalSeconds / 3600
    return if (hours > 0) {
        "%d:%02d:%02d".format(hours, minutes, seconds)
    } else {
        "%d:%02d".format(totalSeconds / 60, seconds)
    }
}
