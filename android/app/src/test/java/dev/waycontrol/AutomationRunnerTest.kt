package dev.waycontrol

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AutomationRunnerTest {
    private val api by lazy { Class.forName("dev.waycontrol.AutomationRunnerKt") }

    @Test
    fun safeActionTypesMapToTheirExistingTypedApiOperations() {
        val actionClass = Class.forName("dev.waycontrol.AutomationAction")
        val typeClass = Class.forName("dev.waycontrol.AutomationActionType")
        val constructor = actionClass.getConstructor(typeClass, String::class.java)
        val requestFor = api.getMethod("automationRequest", actionClass)

        val launch = requestFor.invoke(null, constructor.newInstance(enum(typeClass, "LAUNCH_APPLICATION"), "steam.desktop")) as Pair<*, *>
        assertEquals("app.launch", launch.first)
        assertEquals("steam.desktop", (launch.second as JSONObject).getString("desktop_id"))
        val volume = requestFor.invoke(null, constructor.newInstance(enum(typeClass, "SET_VOLUME"), "35")) as Pair<*, *>
        assertEquals("audio.volume.set", volume.first)
        assertEquals(35, (volume.second as JSONObject).getInt("percent"))
    }

    @Test
    fun confirmationBlocksTheWholeProfileBeforeAnyActionRuns() {
        val profile = profile("Eco")
        val calls = mutableListOf<String>()
        val request: (String, JSONObject) -> JSONObject = { operation, _ -> calls.add(operation); JSONObject() }

        val result = execute(profile, false, request)

        assertTrue(calls.isEmpty())
        assertFalse(result.get("succeeded") as Boolean)
    }

    @Test
    fun actionsRunInOrderAndStopAfterTheFirstFailure() {
        val profile = profile("Eco")
        val calls = mutableListOf<String>()
        val request: (String, JSONObject) -> JSONObject = { operation, _ ->
            calls.add(operation)
            if (operation == "audio.volume.set") error("Audio device unavailable")
            JSONObject()
        }

        val result = execute(profile, true, request)

        assertEquals(listOf("audio.volume.set"), calls)
        assertFalse(result.get("succeeded") as Boolean)
        assertEquals(false, result.getJSONArray("steps").getJSONObject(0).getBoolean("succeeded"))
    }

    private fun profile(name: String): Any {
        val profiles = Class.forName("dev.waycontrol.AutomationProfilesKt")
            .getMethod("defaultAutomationProfiles").invoke(null) as List<*>
        return profiles.first { it!!.property("name") == name }!!
    }

    private fun enum(type: Class<*>, name: String): Any = type.enumConstants!!.first { it.toString() == name }

    private fun execute(profile: Any, confirmed: Boolean, request: (String, JSONObject) -> JSONObject): JSONObject {
        val method = api.methods.first { it.name == "executeAutomationProfile" }
        return method.invoke(null, profile, confirmed, request) as JSONObject
    }

    private fun Any.property(name: String): Any? = javaClass.getMethod("get${name.replaceFirstChar { it.uppercase() }}").invoke(this)
}
