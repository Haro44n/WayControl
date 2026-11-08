package dev.waycontrol

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

class MediaArtworkTest {
    @Test
    fun artworkTargetPrefersPlayingPlayerAndIgnoresMalformedIds() {
        val pausedId = "a".repeat(64)
        val playingId = "b".repeat(64)
        val media = JSONObject().put("players", org.json.JSONArray()
            .put(JSONObject().put("player_id", "paused").put("artwork_id", pausedId))
            .put(JSONObject().put("player_id", "bad").put("artwork_id", "../secret"))
            .put(JSONObject().put("player_id", "playing").put("artwork_id", playingId).put("playback_status", "Playing")))

        assertEquals("playing" to playingId, mediaArtworkTarget(media))
    }

    @Test
    fun artworkDecoderAcceptsSmallImageAndRejectsInvalidPayloads() {
        val id = "c".repeat(64)
        val png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/aO8AAAAASUVORK5CYII="

        assertNotNull(mediaArtworkBytes(JSONObject().put("artwork_id", id).put("data_base64", png)))
        assertNull(mediaArtworkBytes(JSONObject().put("artwork_id", "bad").put("data_base64", png)))
    }
}
