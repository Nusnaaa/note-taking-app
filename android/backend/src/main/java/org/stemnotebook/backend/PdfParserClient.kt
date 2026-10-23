package org.stemnotebook.backend

import android.content.Context
import android.graphics.Bitmap
import android.os.Bundle
import android.os.Handler
import android.os.HandlerThread
import android.os.Message
import android.os.Messenger
import android.os.ParcelFileDescriptor
import java.io.Closeable
import java.io.IOException
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.CancellationException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executor
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.ScheduledThreadPoolExecutor
import java.util.concurrent.ThreadFactory
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
import java.util.concurrent.atomic.AtomicInteger

/** One bounded isolated parser operation at a time. No notebook credentials or
 * source locators are passed to the worker. The caller retains its input FD; this
 * client owns a duplicate. A timeout is not permission to admit another stuck job. */
internal class PdfParserClient(private val connectionFactory: (Executor) -> PdfConnection) : Closeable {
    constructor(context: Context) : this({ executor -> AndroidPdfConnection(context.applicationContext, executor) })
    private val lock = Any()
    private var closed = false
    private var current: Job? = null
    private val next = AtomicInteger(0)
    private fun factory(name: String) = ThreadFactory { Thread(it, name).apply { isDaemon = true } }
    private fun executor(name: String) = ThreadPoolExecutor(1, 1, 0, TimeUnit.MILLISECONDS, ArrayBlockingQueue(1), factory(name), ThreadPoolExecutor.AbortPolicy())
    private val wire = executor("pdf-bind-send")
    private val pixels = executor("pdf-pixels")
    private val cleanup = executor("pdf-dispose")
    private val deadlines = ScheduledThreadPoolExecutor(1, factory("pdf-parser-deadline")).apply { removeOnCancelPolicy = true }
    private val control = HandlerThread("pdf-parser-control").apply { isDaemon = true; start() }
    private val handler = Handler(control.looper)
    private val callbacks = Executor { runnable -> if (!handler.post(runnable)) throw RejectedExecutionException("PDF parser closed") }

    fun render(input: ParcelFileDescriptor, page: Int, width: Int, height: Int, timeoutMillis: Long = 15_000): PendingBitmap {
        PdfPixels.validate(width, height)
        require(page >= 0 && timeoutMillis in 1..120_000)
        val job = synchronized(lock) {
            check(!closed)
            if (current != null) throw RejectedExecutionException("PDF parser busy")
            val token = next.updateAndGet { Math.incrementExact(it) }
            Job(ParcelFileDescriptor.dup(input.fileDescriptor), token, page, width, height).also { current = it }
        }
        job.start(timeoutMillis)
        return PendingBitmap(job)
    }
    internal fun pendingCount(): Int = synchronized(lock) { if (current == null) 0 else 1 }
    private fun forget(job: Job) {
        val stop = synchronized(lock) { if (current === job) current = null; closed && current == null }
        if (stop) control.quitSafely()
    }
    override fun close() {
        val job = synchronized(lock) { if (closed) return; closed = true; current }
        job?.fail(CancellationException("PDF parser closed"))
        wire.shutdown(); pixels.shutdown(); cleanup.shutdown(); deadlines.shutdownNow()
        if (job == null) control.quitSafely()
    }
    class PendingBitmap internal constructor(private val job: Job) : Closeable {
        fun await(): ParsedPage = job.take()
        internal fun awaitRetired() = job.awaitRetired()
        fun cancel() { job.fail(CancellationException("PDF rendering cancelled")) }
        override fun close() = cancel()
    }
    data class ParsedPage(val bitmap: Bitmap, val pages: Int)

    internal inner class Job(private var input: ParcelFileDescriptor?, private val token: Int, private val page: Int, private val width: Int, private val height: Int) {
        private val state = Any()
        private val ready = CountDownLatch(1)
        private val retired = CountDownLatch(1)
        private var done = false
        private var taken = false
        private var reading = false
        private var sent = false
        private var tasks = 0
        private var disposed = false
        private var disposing = false
        private var failure: Exception? = null
        private var result: ParsedPage? = null
        private var pipe: ParcelFileDescriptor? = null
        private var connection: PdfConnection? = null
        private var deadline: ScheduledFuture<*>? = null
        private val reply = Messenger(Handler(control.looper) { receive(it); true })
        fun start(timeoutMillis: Long) {
            try {
                val timer = deadlines.schedule({ fail(TimeoutException("PDF rendering timed out")) }, timeoutMillis, TimeUnit.MILLISECONDS)
                synchronized(state) { if (done) timer.cancel(false) else deadline = timer }
                task(wire, "binding") {
                    val binding = connectionFactory(callbacks)
                    val cancelled = synchronized(state) { if (done) true else { connection = binding; false } }
                    if (cancelled) binding.close()
                    else if (!binding.connect({ connected(it) }, { fail(IOException("PDF parser unavailable")) })) fail(IOException("PDF parser unavailable"))
                }
            } catch (_: RejectedExecutionException) { fail(IOException("PDF parser closed")) }
        }
        private fun task(executor: ThreadPoolExecutor, phase: String, operation: () -> Unit) {
            synchronized(state) { if (done) return; tasks++ }
            try {
                executor.execute {
                    try { operation() } catch (_: Exception) { fail(IOException("PDF parser $phase failed")) }
                    finally { synchronized(state) { tasks-- }; settle() }
                }
            } catch (_: RejectedExecutionException) {
                synchronized(state) { tasks-- }
                fail(IOException("PDF parser capacity reached")); settle()
            }
        }
        private fun connected(remote: Messenger) {
            synchronized(state) { if (done || sent) return; sent = true }
            task(wire, "descriptor send") {
                val fd = synchronized(state) { if (done) null else input } ?: return@task
                try {
                    remote.send(Message.obtain(null, 1).apply {
                        arg1 = page; arg2 = token; replyTo = reply
                        data = Bundle().apply { putParcelable("descriptor", fd); putInt("width", width); putInt("height", height); putString("format", PdfPixels.FORMAT) }
                    })
                } finally {
                    synchronized(state) { if (input === fd) input = null }
                    fd.close()
                }
            }
        }
        @Suppress("DEPRECATION")
        private fun receive(message: Message) {
            var received: ParcelFileDescriptor? = null
            var admitted = false
            try {
                received = message.data.getParcelable("pixels")
                if (message.what != 1 || message.arg2 != token) return
                synchronized(state) { if (done || reading) return; reading = true }
                require(message.data.getBoolean("ok") && message.data.getString("format") == PdfPixels.FORMAT)
                val pages = message.data.getInt("pages")
                require(pages > page)
                val fd = received ?: throw IOException("PDF pixels unavailable")
                synchronized(state) { if (done) return; pipe = fd }
                admitted = true
                task(pixels, "pixel stream") {
                    val bitmap = ParcelFileDescriptor.AutoCloseInputStream(fd).use { input ->
                        PdfPixels.read(input, width, height) { synchronized(state) { if (done) throw CancellationException("PDF rendering cancelled") } }
                    }
                    synchronized(state) { if (pipe === fd) pipe = null }
                    val reject = synchronized(state) {
                        if (done) true else { done = true; result = ParsedPage(bitmap, pages); false }
                    }
                    if (reject) bitmap.recycle() else dispose()
                }
            } catch (_: Exception) { fail(IOException("Invalid PDF parser response")) }
            finally { if (!admitted) try { received?.close() } catch (_: IOException) { } }
        }
        fun fail(error: Exception) {
            val bitmap: Bitmap?
            synchronized(state) {
                if (taken || failure != null) return
                failure = error; done = true
                bitmap = result?.bitmap; result = null
                deadline?.cancel(false)
                ready.countDown()
            }
            bitmap?.recycle()
            dispose()
        }
        private fun dispose() {
            val resources: Triple<PdfConnection?, ParcelFileDescriptor?, ParcelFileDescriptor?>
            synchronized(state) {
                if (disposing) return
                disposing = true
                resources = Triple(connection, input, pipe)
                connection = null; input = null; pipe = null
            }
            // Control publication never waits for a blocked Binder call or pipe read.
            // Keep the admission slot until these tasks and disposal actually finish.
            try {
                cleanup.execute {
                    try {
                        try { resources.first?.close() } finally {
                            try { resources.second?.close() } finally { resources.third?.close() }
                        }
                    } catch (_: Exception) { /* result already terminal; no source details */ }
                    finally { synchronized(state) { disposed = true }; settle() }
                }
            } catch (_: RejectedExecutionException) {
                // close() schedules disposal before shutting the executor down.
                try { resources.first?.close() } finally {
                    try { resources.second?.close() } finally { resources.third?.close() }
                }
                synchronized(state) { disposed = true }
                settle()
            }
        }
        private fun settle() {
            val release = synchronized(state) {
                if (!done || !disposed || tasks != 0) return
                if (failure == null) { deadline?.cancel(false); ready.countDown() }
                failure != null || taken
            }
            if (release) { forget(this); retired.countDown() }
        }
        fun awaitRetired() { retired.await() }
        fun take(): ParsedPage {
            try { ready.await() } catch (interrupted: InterruptedException) {
                fail(CancellationException("PDF rendering interrupted")); Thread.currentThread().interrupt(); throw interrupted
            }
            val value = synchronized(state) {
                failure?.let { throw it }
                check(!taken)
                taken = true
                result!!.also { result = null }
            }
            settle()
            return value
        }
    }
}
