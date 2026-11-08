package dev.waycontrol

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

class AutomationProfilesTest {
    private val api by lazy { Class.forName("dev.waycontrol.AutomationProfilesKt") }

    @Test
    fun defaultProfilesIncludeSupportedStartingTemplates() {
        val profiles = api.getMethod("defaultAutomationProfiles").invoke(null) as List<*>

        assertEquals(listOf("Gaming", "Development", "Away", "Eco", "Custom"), profiles.map { it!!.get("name") })
        assertTrue(profiles.first { it!!.get("name") == "Away" }!!.get("actions").toString().contains("PAUSE_MEDIA"))
        assertTrue(profiles.first { it!!.get("name") == "Away" }!!.get("actions").toString().contains("LOCK_SCREEN"))
        assertTrue(profiles.first { it!!.get("name") == "Eco" }!!.get("actions").toString().contains("SUSPEND"))
    }

    @Test
    fun savedProfilesRoundTripAndRejectUnapprovedActions() {
        val profiles = api.getMethod("defaultAutomationProfiles").invoke(null) as List<*>
        val encode = api.getMethod("encodeAutomationProfiles", List::class.java)
        val decode = api.getMethod("decodeAutomationProfiles", String::class.java)
        val encoded = encode.invoke(null, profiles) as String
        val roundTripped = decode.invoke(null, encoded) as List<*>
        assertEquals(profiles.map { it!!.get("name") }, roundTripped.map { it!!.get("name") })

        val malicious = """[{"id":"custom-1","name":"My setup","icon":"🎮","enabled":false,"requires_confirmation":true,"actions":[{"type":"LAUNCH_APPLICATION","value":"steam.desktop"},{"type":"LAUNCH_APPLICATION","value":"terminal; reboot"}]}]"""
        val loaded = decode.invoke(null, malicious) as List<*>
        assertEquals(false, loaded.single()!!.get("enabled"))
        assertEquals(1, (loaded.single()!!.get("actions") as List<*>).size)
    }

    @Test
    fun suspendAlwaysRequiresConfirmationAndDisabledProfilesCannotRun() {
        val profiles = api.getMethod("defaultAutomationProfiles").invoke(null) as List<*>
        val eco = profiles.first { it!!.get("name") == "Eco" }!!
        val requiresConfirmation = api.getMethod("profileRequiresConfirmation", eco.javaClass)
        val canRun = api.getMethod("profileCanRun", eco.javaClass)

        assertTrue(requiresConfirmation.invoke(null, eco) as Boolean)
        assertTrue(canRun.invoke(null, eco) as Boolean)
        val disabled = api.getMethod("decodeAutomationProfiles", String::class.java).invoke(
            null,
            """[{"id":"custom-1","name":"Disabled","icon":"⚙","enabled":false,"requires_confirmation":false,"actions":[]}]""",
        ) as List<*>
        assertFalse(canRun.invoke(null, disabled.single()) as Boolean)
        assertNotNull(eco)
    }
}

private fun Any.get(property: String): Any? = javaClass.getMethod("get${property.replaceFirstChar { it.uppercase() }}").invoke(this)
