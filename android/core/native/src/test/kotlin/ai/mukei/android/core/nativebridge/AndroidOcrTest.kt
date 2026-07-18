package ai.mukei.android.core.nativebridge

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidOcrTest {
    @Test
    fun boundedRenderSize_scalesUpWithinConfiguredCaps() {
        assertEquals(2000 to 1000, AndroidOcr.boundedRenderSize(1000, 500))
    }

    @Test
    fun boundedRenderSize_capsLargePagesByDimensionAndPixelBudget() {
        val (width, height) = AndroidOcr.boundedRenderSize(6000, 3000)

        assertEquals(2400, width)
        assertEquals(1200, height)
        assertTrue(width.toLong() * height.toLong() <= 4_000_000L)
    }

    @Test
    fun appendBounded_ignoresBlankTextAndAddsPageSeparator() {
        val output = StringBuilder()

        assertFalse(AndroidOcr.appendBounded(output, "   ", 1))
        assertFalse(AndroidOcr.appendBounded(output, " first page ", 1))
        assertFalse(AndroidOcr.appendBounded(output, "second page", 2))

        assertEquals("first page\n\n[Page 2]\nsecond page", output.toString())
    }

    @Test
    fun appendBounded_truncatesAtOcrPayloadLimit() {
        val output = StringBuilder()
        val oversized = "x".repeat(70 * 1024)

        assertTrue(AndroidOcr.appendBounded(output, oversized, 1))
        assertEquals(64 * 1024, output.length)
        assertTrue(AndroidOcr.appendBounded(output, "more", 2))
        assertEquals(64 * 1024, output.length)
    }
}
