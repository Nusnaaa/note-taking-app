package org.stemnotebook.backend

import android.graphics.Bitmap
import android.os.ParcelFileDescriptor
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class PdfWorkerCleanupTest {
    @get:Rule val temporary = TemporaryFolder()
    private fun closed(fd: ParcelFileDescriptor) { assertThrows(IllegalStateException::class.java) { fd.fd } }
    @Test(timeout = 15_000) fun invalidDimensionsCloseInputBeforeParserConstruction() {
        val fd = ParcelFileDescriptor.open(temporary.newFile(), ParcelFileDescriptor.MODE_READ_ONLY)
        var failures = 0
        PdfRenderJob(1, 42, 0, 0, 4096, fd).render {
            assertEquals(42, it.arg2)
            assertFalse(it.data.getBoolean("ok"))
            failures++
        }
        assertEquals(1, failures)
        closed(fd)
    }
    @Test(timeout = 15_000) fun failedReplyClosesBothPipeEnds() {
        val bitmap = Bitmap.createBitmap(2, 2, Bitmap.Config.ARGB_8888)
        val pipe = ParcelFileDescriptor.createPipe()
        try {
            assertThrows(IllegalStateException::class.java) {
                streamPdfPixels(bitmap, pipe) { error("Synthetic Binder reply failure") }
            }
            pipe.forEach { closed(it) }
        } finally { bitmap.recycle(); pipe.forEach { it.close() } }
    }
}
