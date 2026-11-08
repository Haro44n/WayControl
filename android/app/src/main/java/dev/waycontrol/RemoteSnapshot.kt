package dev.waycontrol

import org.json.JSONArray
import org.json.JSONObject

internal data class RemoteSnapshot(
    val desktop: JSONObject = JSONObject(),
    val workspaces: JSONArray = JSONArray(),
    val windows: JSONArray = JSONArray(),
    val monitors: JSONArray = JSONArray(),
    val audio: JSONObject = JSONObject(),
    val media: JSONObject = JSONObject(),
    val networkLatencyMs: Long? = null,
) {
    fun cleared() = RemoteSnapshot()
}
