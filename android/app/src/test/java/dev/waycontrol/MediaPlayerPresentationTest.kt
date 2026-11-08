package dev.waycontrol

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Test

class MediaPlayerPresentationTest {
    @Test
    fun presentationUsesPerPlayerCapabilitiesAndFormatsTrackPosition() {
        val presentation = mediaPlayerPresentation(
            JSONObject()
                .put("playback_status", "Paused")
                .put("can_play", true)
                .put("can_pause", false)
                .put("can_go_previous", false)
                .put("can_go_next", false)
                .put("position_ms", 83_000)
                .put("duration_ms", 245_000),
        )

        assertEquals(false, presentation.canGoPrevious)
        assertEquals(true, presentation.showPlayPause)
        assertEquals("Play", presentation.playPauseLabel)
        assertEquals(false, presentation.canGoNext)
        assertEquals("1:23 / 4:05", presentation.positionText)
    }

    @Test
    fun presentationPreservesLegacyControlsWhenCapabilityFieldsAreMissing() {
        val presentation = mediaPlayerPresentation(JSONObject().put("playback_status", "Playing"))

        assertEquals(true, presentation.canGoPrevious)
        assertEquals(true, presentation.showPlayPause)
        assertEquals("Play / pause", presentation.playPauseLabel)
        assertEquals(true, presentation.canGoNext)
        assertEquals(null, presentation.positionText)
    }

    @Test
    fun presentationHidesEveryActionWhenThePlayerCannotBeControlled() {
        val presentation = mediaPlayerPresentation(
            JSONObject()
                .put("playback_status", "Paused")
                .put("can_control", false)
                .put("can_play", true)
                .put("can_pause", true)
                .put("can_go_previous", true)
                .put("can_go_next", true),
        )

        assertEquals(false, presentation.canGoPrevious)
        assertEquals(false, presentation.showPlayPause)
        assertEquals(false, presentation.canGoNext)
    }
}
