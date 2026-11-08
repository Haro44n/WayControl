package dev.waycontrol

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class RequestEnvelopeTest {
    @Test fun omitsPayloadForUnitOperations() {
        val request = requestEnvelope("desktop.info", "req-1", JSONObject())
        assertEquals(1, request.getInt("version"))
        assertEquals("req-1", request.getString("id"))
        assertEquals("desktop.info", request.getString("type"))
        assertFalse(request.has("payload"))
    }

    @Test fun retainsPayloadForParameterizedOperations() {
        val request = requestEnvelope(
            "workspace.switch",
            "req-2",
            JSONObject().put("workspace_id", 3)
        )
        assertTrue(request.has("payload"))
        assertEquals(3, request.getJSONObject("payload").getInt("workspace_id"))
    }
}
