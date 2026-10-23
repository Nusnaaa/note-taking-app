package org.stemnotebook.backend

import android.content.ComponentName
import android.content.Context
import android.content.ContextWrapper
import android.content.Intent
import android.content.ServiceConnection
import android.os.Handler
import android.os.Looper
import android.os.Message
import android.os.Messenger
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executor
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class PdfConnectionTest {
    @Test fun usesFreshIsolatedBindingWithoutSourceOrNotebookMetadataAndStopsOnClose() {
        val unbound = AtomicInteger(0)
        val stopped = AtomicInteger(0)
        var received: ServiceConnection? = null
        val remote = Messenger(object : Handler(Looper.getMainLooper()) {
            override fun sendMessageAtTime(message: Message, uptimeMillis: Long): Boolean { assertEquals(99, message.what); stopped.incrementAndGet(); return true }
        })
        val context = object : ContextWrapper(RuntimeEnvironment.getApplication()) {
            override fun bindIsolatedService(intent: Intent, flags: Int, instanceName: String, executor: Executor, connection: ServiceConnection): Boolean {
                assertEquals(PdfWorkerService::class.java.name, intent.component!!.className)
                assertNull(intent.data); assertNull(intent.extras)
                assertTrue(flags and Context.BIND_AUTO_CREATE != 0)
                assertTrue(flags and Context.BIND_NOT_FOREGROUND != 0)
                UUID.fromString(instanceName.removePrefix("pdf-"))
                received = connection
                executor.execute { connection.onServiceConnected(intent.component!!, remote.binder) }
                return true
            }
            override fun unbindService(connection: ServiceConnection) { assertSame(received, connection); unbound.incrementAndGet() }
        }
        val ready = AtomicInteger(0)
        val failures = AtomicInteger(0)
        val binding = AndroidPdfConnection(context, Executor { it.run() })
        assertTrue(binding.connect({ ready.incrementAndGet() }, { failures.incrementAndGet() }))
        assertEquals(1, ready.get())
        binding.close(); binding.close()
        assertEquals(1, stopped.get()); assertEquals(1, unbound.get())
        received!!.onServiceDisconnected(ComponentName(context, PdfWorkerService::class.java))
        assertEquals(0, failures.get())
    }
    @Test(timeout = 15_000) fun closeRacingABlockingBindUnbindsItsLateRegistrationExactlyOnce() {
        val entered = CountDownLatch(1)
        val release = CountDownLatch(1)
        val unbound = AtomicInteger(0)
        val context = object : ContextWrapper(RuntimeEnvironment.getApplication()) {
            override fun bindIsolatedService(intent: Intent, flags: Int, instanceName: String, executor: Executor, connection: ServiceConnection): Boolean {
                entered.countDown(); release.await(); return true
            }
            override fun unbindService(connection: ServiceConnection) { unbound.incrementAndGet() }
        }
        val pool = Executors.newSingleThreadExecutor()
        val binding = AndroidPdfConnection(context, Executor { it.run() })
        try {
            val opening = pool.submit<Boolean> { binding.connect({ fail("Late connection") }, { fail("Unexpected failure callback") }) }
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            binding.close(); assertEquals(0, unbound.get())
            release.countDown(); assertTrue(opening.get(5, TimeUnit.SECONDS))
            assertEquals(1, unbound.get())
            binding.close(); assertEquals(1, unbound.get())
        } finally { release.countDown(); binding.close(); pool.shutdownNow() }
    }
}
