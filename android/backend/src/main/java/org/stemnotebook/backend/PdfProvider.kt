package org.stemnotebook.backend

import android.content.ContentResolver
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.CancellationSignal
import android.os.OperationCanceledException
import android.os.ParcelFileDescriptor
import java.io.Closeable
import java.io.IOException
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.CancellationException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.ScheduledThreadPoolExecutor
import java.util.concurrent.ThreadFactory
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException

/** Link-first, bounded provider opening. It obtains no copy or export authority.
 * Timeouts never free a slot while an uncooperative provider is still running.
 * A successful but unclaimed descriptor also retains its slot until taken/closed. */
class PdfProvider internal constructor(
    private val opener: (Uri, CancellationSignal) -> ParcelFileDescriptor,
    private val retainer: (Uri) -> Unit,
    private val revoker: (Uri) -> Unit,
    workers: Int = 2,
    private val maximumPending: Int = 4,
) : Closeable {
    constructor(context: Context) : this(context.applicationContext.contentResolver)
    private constructor(resolver: ContentResolver) : this(
        { uri, signal -> resolver.openFileDescriptor(uri, "r", signal) ?: throw IOException("PDF source unavailable") },
        { uri -> resolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION) },
        { uri -> resolver.releasePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION) },
    )
    private val lock = Any()
    private var closed = false
    private val pending = mutableSetOf<Opening>()
    private fun factory(name: String) = ThreadFactory { runnable -> Thread(runnable, name).apply { isDaemon = true } }
    private val executor: ThreadPoolExecutor
    private val cancellations: ThreadPoolExecutor
    private val deadlines: ScheduledThreadPoolExecutor
    init {
        require(workers in 1..4 && maximumPending in workers..16)
        executor = ThreadPoolExecutor(workers, workers, 0, TimeUnit.MILLISECONDS, ArrayBlockingQueue(maximumPending), factory("pdf-provider"), ThreadPoolExecutor.AbortPolicy())
        cancellations = ThreadPoolExecutor(workers, workers, 0, TimeUnit.MILLISECONDS, ArrayBlockingQueue(maximumPending), factory("pdf-cancel"), ThreadPoolExecutor.AbortPolicy())
        deadlines = ScheduledThreadPoolExecutor(1, factory("pdf-deadline")).apply { removeOnCancelPolicy = true }
    }
    private fun validateUri(uri: Uri) { require(uri.scheme == "content" && uri.toString().length <= 4096) }
    fun retainReadGrant(uri: Uri, pickerFlags: Int) {
        validateUri(uri)
        require(pickerFlags and Intent.FLAG_GRANT_READ_URI_PERMISSION != 0)
        synchronized(lock) { check(!closed) }
        try { retainer(uri) } catch (_: SecurityException) {
            throw SecurityException("PDF read grant unavailable")
        } catch (_: Exception) { throw IOException("PDF read grant failed") }
    }
    fun revoke(uri: Uri) {
        validateUri(uri)
        val affected = synchronized(lock) { pending.filter { it.uri == uri } }
        affected.forEach { it.fail(SecurityException("PDF source access revoked")) }
        try { revoker(uri) } catch (_: SecurityException) {
            throw SecurityException("PDF read grant unavailable")
        } catch (_: Exception) { throw IOException("PDF read grant failed") }
    }
    fun openReadOnly(uri: Uri, timeoutMillis: Long = 15_000): PendingPdf {
        validateUri(uri)
        require(timeoutMillis in 1..120_000)
        val opening = Opening(uri)
        synchronized(lock) {
            check(!closed)
            if (pending.size >= maximumPending) throw RejectedExecutionException("PDF open capacity reached")
            pending += opening
        }
        try {
            opening.attachDeadline(deadlines.schedule({ opening.fail(TimeoutException("PDF source open timed out"), onlyOpening = true) }, timeoutMillis, TimeUnit.MILLISECONDS))
            executor.execute {
                try {
                    if (opening.shouldOpen()) {
                        val descriptor = opener(uri, opening.signal)
                        opening.publish(descriptor)
                    }
                } catch (_: SecurityException) {
                    opening.fail(SecurityException("PDF source access denied"))
                } catch (_: OperationCanceledException) {
                    opening.fail(CancellationException("PDF source open cancelled"))
                } catch (_: Exception) {
                    opening.fail(IOException("PDF source unavailable"))
                } finally { opening.finished() }
            }
        } catch (_: RejectedExecutionException) {
            opening.fail(IOException("PDF provider closed"))
            opening.finished()
        }
        return PendingPdf(opening)
    }
    internal fun pendingCount(): Int = synchronized(lock) { pending.size }
    override fun close() {
        val accepted = synchronized(lock) {
            if (closed) return
            closed = true
            pending.toList()
        }
        accepted.forEach { it.fail(CancellationException("PDF provider closed")) }
        executor.shutdown()
        deadlines.shutdownNow()
        cancellations.shutdown()
    }
    /** Owns a successful descriptor until await transfers it. Close if abandoned.
     * await is blocking and must be called off the Android main thread. No caller
     * completion callback executes on the provider, deadline or control threads. */
    class PendingPdf internal constructor(private val opening: Opening) : Closeable {
        fun await(): ParcelFileDescriptor = opening.take()
        fun cancel() { opening.fail(CancellationException("PDF source open cancelled")) }
        override fun close() = cancel()
    }
    internal inner class Opening(val uri: Uri) {
        val signal = CancellationSignal()
        private val ready = CountDownLatch(1)
        private val state = Any()
        private var active = true
        private var done = false
        private var taken = false
        private var failure: Exception? = null
        private var descriptor: ParcelFileDescriptor? = null
        private var deadline: ScheduledFuture<*>? = null
        fun attachDeadline(timer: ScheduledFuture<*>) = synchronized(state) {
            if (done) timer.cancel(false) else deadline = timer
        }
        fun shouldOpen(): Boolean = synchronized(state) { failure == null }
        fun publish(fd: ParcelFileDescriptor) {
            val rejected = synchronized(state) {
                if (failure != null || taken) true else {
                    descriptor = fd
                    done = true
                    deadline?.cancel(false)
                    ready.countDown()
                    false
                }
            }
            if (rejected) fd.close()
        }
        fun fail(error: Exception, onlyOpening: Boolean = false) {
            val cleanup: Pair<ParcelFileDescriptor?, Boolean>
            val release: Boolean
            synchronized(state) {
                if (taken || failure != null || (onlyOpening && done)) return
                failure = error
                done = true
                cleanup = descriptor to active
                descriptor = null
                deadline?.cancel(false)
                ready.countDown()
                release = !active
            }
            // Cancellation listeners may themselves block. They have a separate,
            // bounded executor and can never occupy the deadline/control thread.
            if (cleanup.second) try { cancellations.execute { signal.cancel() } } catch (_: RejectedExecutionException) { /* bounded shutdown; late results still close */ }
            try { cleanup.first?.close() } catch (_: IOException) { /* terminal ownership remains closed */ } finally { if (release) forget() }
        }
        fun finished() {
            val release = synchronized(state) { active = false; failure != null || taken }
            if (release) forget()
        }
        fun take(): ParcelFileDescriptor {
            try { ready.await() } catch (interrupted: InterruptedException) {
                fail(CancellationException("PDF source open interrupted"))
                Thread.currentThread().interrupt()
                throw interrupted
            }
            val result: ParcelFileDescriptor
            val release: Boolean
            synchronized(state) {
                failure?.let { throw it }
                result = descriptor ?: error("PDF descriptor already taken")
                descriptor = null
                taken = true
                release = !active
            }
            if (release) forget()
            return result
        }
        private fun forget() { synchronized(lock) { pending.remove(this) } }
    }
}
