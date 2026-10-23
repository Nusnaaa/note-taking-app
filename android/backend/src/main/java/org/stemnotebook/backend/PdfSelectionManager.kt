package org.stemnotebook.backend

import android.content.Intent
import android.net.Uri
import android.os.ParcelFileDescriptor
import org.json.JSONObject
import java.io.Closeable
import java.io.IOException
import java.security.MessageDigest
import java.util.UUID
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
import java.util.concurrent.atomic.AtomicBoolean

/** Trusted picker adapter, not a JSON grant flag. Hashing/provider calls run on a
 * bounded optional worker; the native verifier only checks the immutable observation.
 * Source identity is stable across review, including ambiguous commit recovery. */
class PdfSelectionManager internal constructor(
    private val provider: PdfProvider,
    private val prepare: (String, String, String, NativeBridge.PdfGrant) -> JSONObject,
    private val accept: (String, Boolean, NativeBridge.PdfGrant) -> JSONObject,
    private val release: (UUID) -> Unit,
    private val preflight: (Target) -> Unit,
) : Closeable {
    sealed class Target {
        data class Link(val context: UUID) : Target()
        data class Relink(val source: UUID) : Target()
        internal fun json(): JSONObject = when (this) {
            is Link -> JSONObject().put("kind", "Link").put("context", context.toString())
            is Relink -> JSONObject().put("kind", "Relink").put("source", source.toString())
        }
    }
    private val lock = Any()
    private var closed = false
    private var epoch = 0L
    private var running: Operation<*>? = null
    private val reviews = mutableSetOf<Review>()
    private fun factory(name: String) = ThreadFactory { Thread(it, name).apply { isDaemon = true } }
    private val worker = ThreadPoolExecutor(1, 1, 0, TimeUnit.MILLISECONDS, ArrayBlockingQueue(1), factory("pdf-selection"), ThreadPoolExecutor.AbortPolicy())
    private val disposal = ThreadPoolExecutor(1, 1, 0, TimeUnit.MILLISECONDS, ArrayBlockingQueue(1), factory("pdf-selection-dispose"), ThreadPoolExecutor.AbortPolicy())
    private val deadlines = ScheduledThreadPoolExecutor(1, factory("pdf-selection-deadline")).apply { removeOnCancelPolicy = true }
    data class Receipt(val source: UUID)
    class Pending<T> internal constructor(private val operation: Operation<T>) : Closeable {
        fun await(): T = operation.take()
        internal fun awaitRetired() = operation.awaitRetired()
        val isReady: Boolean get() = operation.isReady()
        fun cancel() = operation.cancel(CancellationException("PDF selection cancelled"))
        override fun close() = cancel()
    }
    inner class Review internal constructor(
        internal val target: Target, internal val uri: Uri, internal val flags: Int, internal val digest: String,
        internal val token: UUID, val source: UUID, private val changed: Boolean, private val affected: Int,
        internal val generation: Long,
    ) : Closeable {
        private val born = System.nanoTime()
        internal var consumed = false
        internal fun live(): Boolean = !consumed && System.nanoTime() - born < TimeUnit.MINUTES.toNanos(5)
        val changedRevision: Boolean get() = synchronized(lock) { check(!closed && epoch == generation && live()) { "PDF selection review expired; select again" }; changed }
        val affectedAnchors: Int get() = synchronized(lock) { check(!closed && epoch == generation && live()) { "PDF selection review expired; select again" }; affected }
        override fun close() {
            val remove = synchronized(lock) { if (consumed) false else { consumed = true; reviews.remove(this); true } }
            if (remove) try { release(token) } catch (_: Exception) { /* native cache remains bounded and expires */ }
        }
    }
    fun review(target: Target, uri: Uri, pickerFlags: Int, timeoutMillis: Long = 30_000): Pending<Review> {
        require(uri.scheme == "content" && uri.toString().length <= 4096 && pickerFlags and Intent.FLAG_GRANT_READ_URI_PERMISSION != 0)
        return submit(timeoutMillis, { it.close() }) { operation ->
            synchronized(lock) {
                reviews.removeAll { if (!it.live()) { it.consumed = true; true } else false }
                if (reviews.size >= 32) throw RejectedExecutionException("PDF review capacity reached")
            }
            preflight(target)
            val digest = operation.observe(uri)
            operation.checkpoint()
            val prepared = prepare(target.json().toString(), uri.toString(), digest, operation.proof(target, uri, digest, commit = false))
            val token = UUID.fromString(prepared.getString("review"))
            var owned = true
            try {
                operation.checkpoint()
                val result = Review(target, uri, pickerFlags, digest, token, UUID.fromString(prepared.getString("source")), prepared.getBoolean("changed_revision"), prepared.getInt("affected_anchors"), operation.generation)
                synchronized(lock) {
                    if (closed || epoch != operation.generation) throw CancellationException("PDF selection policy changed")
                    reviews += result
                }
                owned = false
                result
            } finally { if (owned) try { release(token) } catch (_: Exception) { } }
        }
    }
    fun accept(review: Review, timeoutMillis: Long = 30_000): Pending<Receipt> {
        synchronized(lock) { check(!closed && review in reviews && review.generation == epoch && review.live()) }
        return submit(timeoutMillis, {}) { operation ->
            synchronized(lock) {
                check(review in reviews && review.generation == epoch && review.live())
                review.consumed = true; reviews.remove(review)
            }
            var finished = false
            try {
                preflight(review.target)
                if (review.flags and Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION != 0) provider.retainReadGrant(review.uri, review.flags)
                val current = operation.observe(review.uri)
                if (current != review.digest) throw IOException("PDF selection changed; review again")
                operation.checkpoint()
                val source = accept(review.token.toString(), true, operation.proof(review.target, review.uri, current, commit = true))
                finished = true
                require(UUID.fromString(source.getString("id")) == review.source)
                synchronized(lock) { reviews.forEach { it.consumed = true }; reviews.clear() }
                // Return a commit receipt, never a cached source locator/title after relock.
                Receipt(UUID.fromString(source.getString("id")))
            } finally { if (!finished) try { release(review.token) } catch (_: Exception) { } }
        }
    }
    private fun <T> submit(timeoutMillis: Long, discard: (T) -> Unit, body: (Operation<T>) -> T): Pending<T> {
        require(timeoutMillis in 1..120_000)
        val operation = synchronized(lock) {
            check(!closed)
            if (running != null) throw RejectedExecutionException("PDF selection worker busy")
            Operation(timeoutMillis, epoch, discard, body).also { running = it }
        }
        operation.start()
        return Pending(operation)
    }
    fun invalidate() {
        val resources = synchronized(lock) {
            epoch++
            reviews.forEach { it.consumed = true }; reviews.clear()
            running
        }
        resources?.cancel(CancellationException("PDF selection policy changed"))
    }
    override fun close() {
        synchronized(lock) { if (closed) return; closed = true }
        invalidate(); provider.close()
        worker.shutdown(); disposal.shutdown(); deadlines.shutdownNow()
    }
    internal inner class Operation<T>(private val timeoutMillis: Long, val generation: Long, private val discard: (T) -> Unit, private val body: (Operation<T>) -> T) {
        private val state = Any()
        private val ready = CountDownLatch(1)
        private val retired = CountDownLatch(1)
        private val born = System.nanoTime()
        private var failure: Exception? = null
        private var value: T? = null
        private var claimed = false
        private var finished = false
        private var cleanupPending = false
        private var committing = false
        private var timer: ScheduledFuture<*>? = null
        private var opening: PdfProvider.PendingPdf? = null
        private var input: ParcelFileDescriptor? = null
        fun start() {
            try {
                val deadline = deadlines.schedule({ cancel(TimeoutException("PDF selection timed out")) }, timeoutMillis, TimeUnit.MILLISECONDS)
                synchronized(state) { if (failure != null) deadline.cancel(false) else timer = deadline }
                worker.execute {
                    var result: T? = null
                    try {
                        checkpoint(); result = body(this)
                        synchronized(state) {
                            if (failure == null) { value = result; result = null }
                        }
                    } catch (error: Exception) {
                        synchronized(state) {
                            if (failure == null) failure = when (error) {
                                is TimeoutException -> TimeoutException("PDF selection timed out")
                                is CancellationException -> CancellationException("PDF selection cancelled")
                                else -> IOException("PDF selection unavailable or changed; review again")
                            }
                            // Failure publication must not wait for an uncooperative
                            // provider to retire or for our own deadline thread.
                            ready.countDown()
                        }
                    } finally {
                        result?.let { discard(it) }
                        val resources = synchronized(state) { val pair = opening to input; opening = null; input = null; pair }
                        try { resources.first?.close() } catch (_: Exception) { }
                        try { resources.second?.close() } catch (_: Exception) { }
                        try { resources.first?.awaitRetired() } catch (_: InterruptedException) { Thread.currentThread().interrupt() }
                        synchronized(state) { finished = true; timer?.cancel(false) }
                        retire(); ready.countDown()
                    }
                }
            } catch (_: RejectedExecutionException) {
                cancel(IOException("PDF selection closed")); synchronized(state) { finished = true }; retire()
            }
        }
        fun checkpoint() {
            synchronized(state) { failure?.let { throw it }; if (committing) return }
            synchronized(lock) { if (closed || epoch != generation) throw CancellationException("PDF selection policy changed") }
            if (System.nanoTime() - born >= TimeUnit.MILLISECONDS.toNanos(timeoutMillis)) throw TimeoutException("PDF selection timed out")
        }
        fun observe(uri: Uri): String {
            checkpoint()
            val remaining = maxOf(1, timeoutMillis - TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - born))
            val pending = provider.openReadOnly(uri, remaining)
            val cancelled = synchronized(state) { opening = pending; failure != null }
            if (cancelled) pending.cancel()
            checkpoint()
            val fd = pending.await()
            synchronized(state) { opening = null; input = fd }
            checkpoint()
            val digest = MessageDigest.getInstance("SHA-256")
            val prefix = ByteArray(5); var seen = 0; var bytes = 0L
            ParcelFileDescriptor.AutoCloseInputStream(fd).use { stream ->
                val buffer = ByteArray(65536)
                while (true) {
                    checkpoint()
                    val count = stream.read(buffer); if (count < 0) break
                    bytes += count
                    if (bytes > 16L * 1024 * 1024 * 1024) throw IOException("PDF selection resource limit")
                    val first = minOf(count, prefix.size - seen)
                    if (first > 0) { buffer.copyInto(prefix, seen, 0, first); seen += first }
                    if (seen == prefix.size && !prefix.contentEquals("%PDF-".toByteArray(Charsets.US_ASCII))) throw IOException("Unsupported PDF source")
                    digest.update(buffer, 0, count)
                }
            }
            synchronized(state) { input = null }
            if (!prefix.contentEquals("%PDF-".toByteArray(Charsets.US_ASCII))) throw IOException("Unsupported PDF source")
            checkpoint()
            return digest.digest().joinToString("") { "%02x".format(it.toInt() and 255) }
        }
        fun proof(target: Target, uri: Uri, digest: String, commit: Boolean): NativeBridge.PdfGrant {
            val used = AtomicBoolean(false)
            return NativeBridge.PdfGrant { challenge, requestedTarget, locator, revision ->
                try {
                    UUID.fromString(challenge)
                    val requested = JSONObject(requestedTarget)
                    val expected = target.json()
                    if (requested.length() != expected.length() || expected.keys().asSequence().any { requested.optString(it) != expected.getString(it) } || locator != uri.toString() || revision != digest || !used.compareAndSet(false, true)) false
                    else {
                        checkpoint()
                        synchronized(lock) {
                            synchronized(state) {
                                if (closed || epoch != generation || failure != null || claimed || System.nanoTime() - born >= TimeUnit.MILLISECONDS.toNanos(timeoutMillis)) false else {
                                    if (commit) { committing = true; timer?.cancel(false) }
                                    true
                                }
                            }
                        }
                    }
                } catch (_: Exception) { false }
            }
        }
        fun cancel(error: Exception) {
            val resources: Pair<PdfProvider.PendingPdf?, ParcelFileDescriptor?>
            val result: T?
            synchronized(state) {
                if (failure != null || claimed || committing) return
                failure = error; result = value; value = null
                timer?.cancel(false); resources = opening to input
                cleanupPending = resources.first != null || resources.second != null
                ready.countDown()
            }
            result?.let { discard(it) }
            if (!cleanupPending) { retire(); return }
            fun closeResources() {
                try { try { resources.first?.cancel() } finally { resources.second?.close() } } catch (_: Exception) { }
                finally { synchronized(state) { cleanupPending = false }; retire() }
            }
            try { disposal.execute { closeResources() } } catch (_: RejectedExecutionException) { closeResources() }
        }
        private fun retire() {
            val complete = synchronized(state) { finished && !cleanupPending }
            if (complete) {
                synchronized(lock) { if (running === this) running = null }
                retired.countDown()
            }
        }
        fun awaitRetired() { retired.await() }
        fun isReady() = ready.count == 0L
        fun take(): T {
            try { ready.await() } catch (interrupted: InterruptedException) {
                cancel(CancellationException("PDF selection interrupted")); Thread.currentThread().interrupt(); throw interrupted
            }
            return synchronized(state) {
                failure?.let { throw it }; check(finished && !claimed); claimed = true
                @Suppress("UNCHECKED_CAST") (value as T).also { value = null }
            }
        }
    }
}
