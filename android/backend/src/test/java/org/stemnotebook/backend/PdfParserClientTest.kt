package org.stemnotebook.backend

import android.graphics.Bitmap
import android.os.*
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.io.IOException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executor
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
import java.util.concurrent.atomic.AtomicInteger

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class PdfParserClientTest {
    @get:Rule val temporary = TemporaryFolder()
    private fun input() = ParcelFileDescriptor.open(temporary.newFile(), ParcelFileDescriptor.MODE_READ_ONLY)
    private fun eventually(condition: () -> Boolean) {
        val limit = System.nanoTime() + TimeUnit.SECONDS.toNanos(5)
        while (!condition() && System.nanoTime() < limit) Thread.sleep(5)
        assertTrue(condition())
    }
    @Test(timeout = 15_000) fun realMessengerDispatchReturnsOnlyCompleteBoundedPixelsAndClosesResources() {
        lateinit var binding: FixturePdfConnection
        val client = PdfParserClient { callbacks -> FixturePdfConnection(callbacks, "ok").also { binding = it } }
        input().use { original ->
            client.use {
                val result = client.render(original, 1, 2, 2, 5_000).await()
                try { assertEquals(3, result.pages); assertEquals(0xff112233.toInt(), result.bitmap.getPixel(0, 0)) } finally { result.bitmap.recycle() }
                eventually { client.pendingCount() == 0 }
                assertTrue(original.fd >= 0)
                assertEquals(1, binding.closes.get())
                assertThrows(IllegalStateException::class.java) { binding.output!!.fd }
            }
        }
    }
    @Test(timeout = 15_000) fun timeoutCannotRefillAStillBlockedBinderSendSlot() {
        lateinit var binding: FixturePdfConnection
        val created = CountDownLatch(1)
        val client = PdfParserClient { callbacks -> FixturePdfConnection(callbacks, "blocked-send").also { binding = it; created.countDown() } }
        input().use { fd ->
            try {
                val request = client.render(fd, 0, 2, 2, 1_000)
                assertTrue(created.await(5, TimeUnit.SECONDS))
                assertTrue(binding.entered.await(5, TimeUnit.SECONDS))
                assertThrows(TimeoutException::class.java) { request.await() }
                assertEquals(1, client.pendingCount())
                assertThrows(RejectedExecutionException::class.java) { client.render(fd, 0, 2, 2) }
                binding.release.countDown()
                eventually { client.pendingCount() == 0 }
                assertTrue(fd.fd >= 0)
            } finally { binding.release.countDown(); client.close() }
        }
    }
    @Test(timeout = 15_000) fun malformedPixelsAndBinderDeathFailWithoutPublishingABitmap() {
        for (mode in listOf("truncated", "death")) {
            lateinit var binding: FixturePdfConnection
            val created = CountDownLatch(1)
            PdfParserClient { callbacks -> FixturePdfConnection(callbacks, mode).also { binding = it; created.countDown() } }.use { client ->
                input().use { fd ->
                    val pending = client.render(fd, 0, 2, 2, 5_000)
                    if (mode == "death") { assertTrue(created.await(5, TimeUnit.SECONDS)); assertTrue(binding.entered.await(5, TimeUnit.SECONDS)); binding.failed!!() }
                    assertThrows(IOException::class.java) { pending.await() }
                    eventually { client.pendingCount() == 0 }
                    assertEquals(1, binding.closes.get())
                }
            }
        }
    }
    @Test(timeout = 15_000) fun wrongOrLateRepliesCannotPublishAndTheirDescriptorsClose() {
        lateinit var binding: FixturePdfConnection
        PdfParserClient { callbacks -> FixturePdfConnection(callbacks, "wrong-token").also { binding = it } }.use { client ->
            input().use { fd ->
                val request = client.render(fd, 0, 2, 2, 1_000)
                assertThrows(TimeoutException::class.java) { request.await() }
                eventually { client.pendingCount() == 0 }
                assertThrows(IllegalStateException::class.java) { binding.output!!.fd }
            }
        }
    }
    @Test(timeout = 15_000) fun expiredBindAndExplicitCancelRejectLateConnectionsWithoutSendingInput() {
        for (cancel in listOf(false, true)) {
            lateinit var binding: FixturePdfConnection
            val created = CountDownLatch(1)
            PdfParserClient { callbacks -> FixturePdfConnection(callbacks, "never-bind").also { binding = it; created.countDown() } }.use { client ->
                input().use { fd ->
                    val pending = client.render(fd, 0, 2, 2, 1_000)
                    assertTrue(created.await(5, TimeUnit.SECONDS))
                    if (cancel) {
                        pending.cancel()
                        assertThrows(java.util.concurrent.CancellationException::class.java) { pending.await() }
                    } else assertThrows(TimeoutException::class.java) { pending.await() }
                    eventually { client.pendingCount() == 0 }
                    binding.connected!!(binding.messenger)
                    assertEquals(1L, binding.entered.count)
                    assertEquals(1, binding.closes.get())
                    assertTrue(fd.fd >= 0)
                }
            }
        }
    }

}
