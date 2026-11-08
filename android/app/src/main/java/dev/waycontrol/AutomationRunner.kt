package dev.waycontrol

import org.json.JSONArray
import org.json.JSONObject

internal fun automationRequest(action: AutomationAction): Pair<String, JSONObject>? {
    val safe = sanitizeAutomationAction(action) ?: return null
    return when (safe.type) {
        AutomationActionType.LAUNCH_APPLICATION -> "app.launch" to JSONObject().put("desktop_id", safe.value)
        AutomationActionType.SWITCH_WORKSPACE -> "workspace.switch" to JSONObject().put("workspace_id", safe.value)
        AutomationActionType.SET_VOLUME -> "audio.volume.set" to JSONObject().put("percent", safe.value.toInt())
        AutomationActionType.LOCK_SCREEN -> "system.lock" to JSONObject()
        AutomationActionType.SUSPEND -> "system.power" to JSONObject().put("action", "suspend")
        AutomationActionType.PAUSE_MEDIA -> null
    }
}

internal fun executeAutomationProfile(
    profile: AutomationProfile,
    confirmed: Boolean,
    request: (String, JSONObject) -> JSONObject,
): JSONObject {
    val steps = JSONArray()
    if (!profileCanRun(profile)) {
        return JSONObject().put("profile_id", profile.id).put("succeeded", false)
            .put("summary", if (!profile.enabled) "This profile is disabled" else "This profile has no actions")
            .put("steps", steps)
    }
    if (profileRequiresConfirmation(profile) && !confirmed) {
        return JSONObject().put("profile_id", profile.id).put("succeeded", false)
            .put("summary", "Confirmation is required").put("steps", steps)
    }

    var succeeded = true
    for (action in profile.actions) {
        val step = JSONObject().put("action", actionDescription(action))
        try {
            val message = if (action.type == AutomationActionType.PAUSE_MEDIA) {
                pauseCurrentMedia(request)
            } else {
                val (operation, payload) = automationRequest(action)
                    ?: error("This action is not supported")
                request(operation, payload)
                actionDescription(action)
            }
            step.put("succeeded", true).put("message", message)
        } catch (error: Exception) {
            succeeded = false
            step.put("succeeded", false).put("message", error.message ?: "Action failed")
        }
        steps.put(step)
        if (!succeeded) break
    }
    return JSONObject()
        .put("profile_id", profile.id)
        .put("succeeded", succeeded)
        .put("summary", if (succeeded) "Completed ${steps.length()} action(s)" else "Stopped after an action failed")
        .put("steps", steps)
}

private fun pauseCurrentMedia(request: (String, JSONObject) -> JSONObject): String {
    val players = request("media.info", JSONObject()).optJSONArray("players") ?: JSONArray()
    val playing = (0 until players.length())
        .mapNotNull(players::optJSONObject)
        .firstOrNull { it.optString("playback_status").equals("Playing", ignoreCase = true) }
        ?: return "No media is playing"
    if (playing.has("can_pause") && !playing.isNull("can_pause") && !playing.optBoolean("can_pause")) {
        error("The active player does not support pause")
    }
    val playerId = playing.optString("player_id").takeIf(String::isNotBlank)
        ?: error("The active player has no usable ID")
    request("media.play_pause_for", JSONObject().put("player_id", playerId))
    return "Paused ${playing.optString("name", playerId)}"
}

private fun actionDescription(action: AutomationAction): String = when (action.type) {
    AutomationActionType.LAUNCH_APPLICATION -> "Launch ${action.value}"
    AutomationActionType.SWITCH_WORKSPACE -> "Switch to workspace ${action.value}"
    AutomationActionType.SET_VOLUME -> "Set output volume to ${action.value}%"
    AutomationActionType.PAUSE_MEDIA -> "Pause media"
    AutomationActionType.LOCK_SCREEN -> "Lock screen"
    AutomationActionType.SUSPEND -> "Suspend computer"
}
