package dev.waycontrol

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

internal enum class AutomationActionType(val label: String, val needsValue: Boolean = false) {
    LAUNCH_APPLICATION("Launch application", true),
    SWITCH_WORKSPACE("Switch workspace", true),
    SET_VOLUME("Set volume", true),
    PAUSE_MEDIA("Pause media"),
    LOCK_SCREEN("Lock screen"),
    SUSPEND("Suspend computer"),
}

internal data class AutomationAction(
    val type: AutomationActionType,
    val value: String = "",
)

internal data class AutomationProfile(
    val id: String,
    val name: String,
    val icon: String,
    val actions: List<AutomationAction>,
    val enabled: Boolean = true,
    val requiresConfirmation: Boolean = false,
)

internal fun defaultAutomationProfiles(): List<AutomationProfile> = listOf(
    AutomationProfile("gaming", "Gaming", "🎮", emptyList()),
    AutomationProfile("development", "Development", "💻", emptyList()),
    AutomationProfile(
        "away", "Away", "🌙",
        listOf(AutomationAction(AutomationActionType.PAUSE_MEDIA), AutomationAction(AutomationActionType.LOCK_SCREEN)),
    ),
    AutomationProfile(
        "eco", "Eco", "🌿",
        listOf(AutomationAction(AutomationActionType.SET_VOLUME, "20"), AutomationAction(AutomationActionType.SUSPEND)),
        requiresConfirmation = true,
    ),
    AutomationProfile("custom", "Custom", "⚙️", emptyList()),
)

internal fun profileRequiresConfirmation(profile: AutomationProfile): Boolean =
    profile.requiresConfirmation || profile.actions.any { it.type == AutomationActionType.SUSPEND }

internal fun profileCanRun(profile: AutomationProfile): Boolean = profile.enabled && profile.actions.isNotEmpty()

internal fun encodeAutomationProfiles(profiles: List<AutomationProfile>): String = JSONArray().apply {
    profiles.take(MAX_AUTOMATION_PROFILES).forEach { profile ->
        val safe = sanitizeAutomationProfile(profile) ?: return@forEach
        put(JSONObject()
            .put("id", safe.id)
            .put("name", safe.name)
            .put("icon", safe.icon)
            .put("enabled", safe.enabled)
            .put("requires_confirmation", safe.requiresConfirmation)
            .put("actions", JSONArray().apply {
                safe.actions.forEach { action ->
                    put(JSONObject().put("type", action.type.name).put("value", action.value))
                }
            }))
    }
}.toString()

internal fun decodeAutomationProfiles(encoded: String): List<AutomationProfile> {
    val json = runCatching { JSONArray(encoded) }.getOrNull() ?: return defaultAutomationProfiles()
    return buildList {
        for (index in 0 until minOf(json.length(), MAX_AUTOMATION_PROFILES)) {
            val profileJson = json.optJSONObject(index) ?: continue
            val id = profileJson.optString("id")
            val name = profileJson.optString("name")
            val icon = profileJson.optString("icon")
            val actionsJson = profileJson.optJSONArray("actions") ?: JSONArray()
            val actions = buildList {
                for (actionIndex in 0 until minOf(actionsJson.length(), MAX_AUTOMATION_ACTIONS)) {
                    val item = actionsJson.optJSONObject(actionIndex) ?: continue
                    val type = runCatching { AutomationActionType.valueOf(item.optString("type")) }.getOrNull() ?: continue
                    sanitizeAutomationAction(AutomationAction(type, item.optString("value")))?.let(::add)
                }
            }
            val profile = AutomationProfile(
                id = id,
                name = name,
                icon = icon,
                actions = actions,
                enabled = profileJson.optBoolean("enabled", true),
                requiresConfirmation = profileJson.optBoolean("requires_confirmation", false),
            )
            sanitizeAutomationProfile(profile)?.let(::add)
        }
    }
}

private const val MAX_AUTOMATION_PROFILES = 20
private const val MAX_AUTOMATION_ACTIONS = 32

internal fun sanitizeAutomationProfile(profile: AutomationProfile): AutomationProfile? {
    val id = profile.id.trim()
    val name = profile.name.trim()
    val icon = profile.icon.trim()
    if (!id.matches(Regex("[A-Za-z0-9_-]{1,64}")) ||
        name.isEmpty() || name.length > 48 || name.any(Char::isISOControl) ||
        icon.isEmpty() || icon.length > 12 || icon.any(Char::isISOControl) ||
        profile.actions.size > MAX_AUTOMATION_ACTIONS
    ) return null
    val actions = profile.actions.mapNotNull(::sanitizeAutomationAction)
    return profile.copy(id = id, name = name, icon = icon, actions = actions)
}

internal fun sanitizeAutomationAction(action: AutomationAction): AutomationAction? {
    val value = action.value.trim()
    return when (action.type) {
        AutomationActionType.LAUNCH_APPLICATION -> if (
            value.length in 1..256 && value.matches(Regex("[A-Za-z0-9][A-Za-z0-9_.-]*"))
        ) action.copy(value = value) else null
        AutomationActionType.SWITCH_WORKSPACE -> if (
            value.length in 1..128 && !value.any(Char::isISOControl)
        ) action.copy(value = value) else null
        AutomationActionType.SET_VOLUME -> value.toIntOrNull()?.takeIf { it in 0..100 }?.let { action.copy(value = it.toString()) }
        AutomationActionType.PAUSE_MEDIA,
        AutomationActionType.LOCK_SCREEN,
        AutomationActionType.SUSPEND -> action.copy(value = "")
    }
}

internal class AutomationProfileStore(context: Context) {
    private val preferences = context.getSharedPreferences("automation_profiles", Context.MODE_PRIVATE)

    fun load(): List<AutomationProfile> = preferences.getString("profiles", null)
        ?.let(::decodeAutomationProfiles)
        ?: defaultAutomationProfiles()

    fun save(profiles: List<AutomationProfile>) {
        preferences.edit().putString("profiles", encodeAutomationProfiles(profiles)).apply()
    }
}
