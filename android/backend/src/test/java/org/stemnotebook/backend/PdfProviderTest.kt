package org.stemnotebook.backend

import android.net.Uri
import android.os.ParcelFileDescriptor
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.io.IOException
import java.util.concurrent.CancellationException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
import java.util.concurrent.atomic.AtomicReference

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class PdfProviderTest {
    @get:Rule val temporary = TemporaryFolder()
    private val uri = Uri.parse("content://synthetic/source")
    private fun descriptor(): ParcelFileDescriptor = ParcelFileDescriptor.open(temporary.newFile(), ParcelFileDescriptor.MODE_READ_ONLY)
    private fun eventually(condition: () -> Boolean) {
        val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(5)
        while (!condition() && System.nanoTime() < deadline) Thread.sleep(5)
        assertTrue(condition())
    }
    private fun closed(fd: ParcelFileDescriptor) {
        assertThrows(IllegalStateException::class.java) { fd.fd }
    }
    @Test(timeout = 15_000) fun openingIsOffThreadAndTransfersOneReadOnlyDescriptor() {
        val thread = AtomicReference<Thread>()
        val fd = descriptor()
        PdfProvider({ requested, _ -> assertEquals(uri, requested); thread.set(Thread.currentThread()); fd }, {}, {}, workers = 1, maximumPending = 1).use { provider ->
            provider.openReadOnly(uri).use { pending ->
                val received = pending.await()
                assertSame(fd, received)
                assertNotSame(Thread.currentThread(), thread.get())
                assertThrows(IllegalStateException::class.java) { pending.await() }
                pending.close()
                assertTrue(received.fd >= 0) // transferred ownership belongs to the caller
                received.close()
            }
            eventually { provider.pendingCount() == 0 }
        }
    }
    @Test(timeout = 15_000) fun timeoutDoesNotFreeUncooperativeProviderAndLateDescriptorCloses() {
        val entered = CountDownLatch(1)
        val release = CountDownLatch(1)
        val fd = descriptor()
        val provider = PdfProvider({ _, _ -> entered.countDown(); release.await(); fd }, {}, {}, workers = 1, maximumPending = 1)
        try {
            val first = provider.openReadOnly(uri, 1_000)
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            assertThrows(TimeoutException::class.java) { first.await() }
            assertEquals(1, provider.pendingCount())
            repeat(8) { assertThrows(RejectedExecutionException::class.java) { provider.openReadOnly(uri) } }
            release.countDown()
            eventually { provider.pendingCount() == 0 }
            closed(fd)
        } finally { release.countDown(); provider.close() }
    }
    @Test(timeout = 15_000) fun cancellationListenerCannotBlockOtherDeadlinesAndCancelledQueueNeverOpens() {
        val entered = CountDownLatch(1)
        val cancelEntered = CountDownLatch(1)
        val release = CountDownLatch(1)
        val fd = descriptor()
        val provider = PdfProvider({ _, signal ->
            signal.setOnCancelListener { cancelEntered.countDown(); release.await() }
            entered.countDown(); release.await(); fd
        }, {}, {}, workers = 1, maximumPending = 2)
        try {
            val first = provider.openReadOnly(uri, 1_000)
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            assertThrows(TimeoutException::class.java) { first.await() }
            assertTrue(cancelEntered.await(5, TimeUnit.SECONDS))
            val second = provider.openReadOnly(uri, 1_000)
            assertThrows(TimeoutException::class.java) { second.await() }
            assertEquals(2, provider.pendingCount())
            release.countDown()
            eventually { provider.pendingCount() == 0 }
            closed(fd)
        } finally { release.countDown(); provider.close() }
    }
    @Test(timeout = 15_000) fun abandoningReadyDescriptorAndProviderCloseReleaseOwnership() {
        val ready = CountDownLatch(1)
        val fd = descriptor()
        val provider = PdfProvider({ _, _ -> ready.countDown(); fd }, {}, {}, workers = 1, maximumPending = 1)
        val pending = provider.openReadOnly(uri)
        assertTrue(ready.await(5, TimeUnit.SECONDS))
        // Slot includes ready but unclaimed descriptors.
        assertThrows(RejectedExecutionException::class.java) { provider.openReadOnly(uri) }
        provider.close()
        assertThrows(CancellationException::class.java) { pending.await() }
        eventually { provider.pendingCount() == 0 }
        closed(fd)
        provider.close()
        assertThrows(IllegalStateException::class.java) { provider.openReadOnly(uri) }
    }
    @Test(timeout = 15_000) fun deniedAndFailedProviderErrorsAreContentFree() {
        val provider = PdfProvider({ _, _ -> throw SecurityException("private-provider-title") }, {}, {})
        provider.use {
            val error = assertThrows(SecurityException::class.java) { it.openReadOnly(uri).await() }
            assertFalse(error.message!!.contains("private")); assertNull(error.cause)
        }
        PdfProvider({ _, _ -> throw IOException("private-locator") }, {}, {}).use {
            val error = assertThrows(IOException::class.java) { it.openReadOnly(uri).await() }
            assertFalse(error.message!!.contains("private")); assertNull(error.cause)
        }
    }
    @Test(timeout = 15_000) fun explicitCancellationAndGrantRevocationDiscardLateResults() {
        val entered = CountDownLatch(1)
        val release = CountDownLatch(1)
        val revoked = AtomicReference<Uri>()
        val fd = descriptor()
        val provider = PdfProvider({ _, _ -> entered.countDown(); release.await(); fd }, {}, { revoked.set(it) }, workers = 1, maximumPending = 1)
        try {
            val pending = provider.openReadOnly(uri)
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            provider.revoke(uri)
            assertEquals(uri, revoked.get())
            assertThrows(SecurityException::class.java) { pending.await() }
            pending.cancel() // idempotent and must not replace the revocation reason
            release.countDown()
            eventually { provider.pendingCount() == 0 }
            closed(fd)
        } finally { release.countDown(); provider.close() }
    }
    @Test(timeout = 15_000) fun explicitCancelSignalsProviderWithoutFreeingItsSlotEarly() {
        val entered = CountDownLatch(1)
        val signalled = CountDownLatch(1)
        val release = CountDownLatch(1)
        val fd = descriptor()
        val provider = PdfProvider({ _, signal ->
            signal.setOnCancelListener { signalled.countDown() }
            entered.countDown(); release.await(); fd
        }, {}, {}, workers = 1, maximumPending = 1)
        try {
            val pending = provider.openReadOnly(uri)
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            pending.cancel()
            assertThrows(CancellationException::class.java) { pending.await() }
            assertTrue(signalled.await(5, TimeUnit.SECONDS))
            assertEquals(1, provider.pendingCount())
            release.countDown()
            eventually { provider.pendingCount() == 0 }
            closed(fd)
        } finally { release.countDown(); provider.close() }
    }
    @Test(timeout = 15_000) fun malformedAcquisitionDoesNotEnterProviderOrRetainAnImplicitGrant() {
        PdfProvider({ _, _ -> fail("Unexpected acquisition"); descriptor() }, { fail("Unexpected grant") }, { fail("Unexpected revoke") }).use { provider ->
            assertThrows(IllegalArgumentException::class.java) { provider.openReadOnly(Uri.parse("https://example.invalid/private")) }
            assertThrows(IllegalArgumentException::class.java) { provider.openReadOnly(Uri.parse("content://synthetic/" + "x".repeat(4096))) }
            assertThrows(IllegalArgumentException::class.java) { provider.openReadOnly(uri, 0) }
            assertThrows(IllegalArgumentException::class.java) { provider.retainReadGrant(uri, 0) }
            assertEquals(0, provider.pendingCount())
        }
    }
}
