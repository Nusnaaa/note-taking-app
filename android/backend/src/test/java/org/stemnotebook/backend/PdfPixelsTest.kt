package org.stemnotebook.backend

import android.graphics.Bitmap
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.DataOutputStream
import java.io.IOException

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class PdfPixelsTest {
    private fun fixture(): ByteArray {
        val bitmap = Bitmap.createBitmap(2, 2, Bitmap.Config.ARGB_8888)
        try {
            bitmap.setPixels(intArrayOf(0xff112233.toInt(), 0xffabcdef.toInt(), 0xff000000.toInt(), -1), 0, 2, 0, 0, 2, 2)
            return ByteArrayOutputStream().also { PdfPixels.write(bitmap, it) }.toByteArray()
        } finally { bitmap.recycle() }
    }
    @Test fun exactArgbRowsRoundTripWithoutAnImageDecoder() {
        val bytes = fixture()
        assertEquals(16 + 2 * 2 * 4, bytes.size)
        val bitmap = PdfPixels.read(ByteArrayInputStream(bytes), 2, 2)
        try { assertEquals(0xff112233.toInt(), bitmap.getPixel(0, 0)); assertEquals(-1, bitmap.getPixel(1, 1)) }
        finally { bitmap.recycle() }
    }
    @Test fun malformedTruncatedOversizedAndTrailingFramesFail() {
        val bytes = fixture()
        assertThrows(IOException::class.java) { PdfPixels.read(ByteArrayInputStream(bytes.copyOf(7)), 2, 2) }
        assertThrows(IOException::class.java) { PdfPixels.read(ByteArrayInputStream(bytes.copyOf(bytes.size - 1)), 2, 2) }
        assertThrows(IOException::class.java) { PdfPixels.read(ByteArrayInputStream(bytes + byteArrayOf(0)), 2, 2) }
        assertThrows(IOException::class.java) { PdfPixels.read(ByteArrayInputStream(bytes), 1, 2) }
        val malicious = ByteArrayOutputStream().also { DataOutputStream(it).apply { write("STEMPIX1".toByteArray()); writeInt(Int.MAX_VALUE); writeInt(Int.MAX_VALUE) } }.toByteArray()
        assertThrows(IOException::class.java) { PdfPixels.read(ByteArrayInputStream(malicious), 2, 2) }
        assertThrows(IllegalArgumentException::class.java) { PdfPixels.read(ByteArrayInputStream(bytes), 4096, 4096) }
    }
    @Test fun cancellationDuringRowsFailsWithoutPublishingAPartialBitmap() {
        var checkpoints = 0
        assertThrows(IllegalStateException::class.java) {
            PdfPixels.read(ByteArrayInputStream(fixture()), 2, 2) { if (++checkpoints == 2) error("Synthetic cancellation") }
        }
        assertEquals(2, checkpoints)
    }
}
