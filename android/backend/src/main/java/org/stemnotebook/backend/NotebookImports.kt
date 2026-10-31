package org.stemnotebook.backend

import android.net.Uri
import android.os.CancellationSignal
import android.os.Looper
import org.json.JSONObject
import java.io.Closeable
import java.io.InputStream
import java.util.Collections
import java.util.UUID
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.CancellationException
import java.util.concurrent.CompletableFuture
import java.util.concurrent.ExecutionException
import java.util.concurrent.ScheduledThreadPoolExecutor
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit

/** One bounded optional caller-worker for deliberate native copies. Provider I/O,
 * credentials and platform callbacks never run on the authoring executor. */
class NotebookImports internal constructor(private val nativeHandle: () -> Long, private val documents: ImportDocuments,
    private val fault: (String) -> Unit = {}, private val admission: () -> Unit = {}) : Closeable {
    enum class FailureReason { Unavailable, Cancelled, Busy, Closed, InvalidInput }
    /** Once publication is attempted, failure never proves rollback. Retain the
     * operation ID, retire pending work, then reconcile against the same library. */
    class Failure(val reason: FailureReason, val operationId: UUID? = null, val publicationMayHaveCommitted: Boolean = false) :
        IllegalStateException("Native import operation unavailable")
    data class Limits(val inputBytes: Int, val timeoutMillis: Long) {
        init { require(inputBytes in 1..MAX_INPUT_BYTES && timeoutMillis in 1..3_600_000) }
    }
    /** Source omission strings are untrusted artifact metadata, not permission. */
    data class Summary(val sourceVersion: Int, val entities: Long, val blocks: Long, val strokes: Long,
        val historyEntries: Long, val protectedObjects: Long, val retainedTitles: Long, val sourceOmissions: List<String>)
    data class Receipt(val operation: UUID, val artifactDigest: String, val entities: List<UUID>, val completedMillis: Long)
    /** Absence is only an observation, not retry authorization while the original
     * publication may still be running. */
    data class Reconciliation(val receipt: Receipt?)
    class Pending<T : Any> internal constructor(private val result: CompletableFuture<T>, private val cancelAction: () -> Boolean,
        private val observer: () -> Unit, private val identity: () -> UUID?, private val attempted: () -> Boolean) : Closeable {
        val operationId: UUID? get() = identity()
        val isReady: Boolean get() = result.isDone
        fun await(): T {
            observer()
            return try { result.get() }
            catch (error: InterruptedException) { cancel(); Thread.currentThread().interrupt(); throw Failure(FailureReason.Cancelled, operationId, attempted()) }
            catch (error: ExecutionException) { throw error.cause as? Failure ?: Failure(FailureReason.Unavailable, operationId, attempted()) }
        }
        /** Cooperative request only. A known committed result stays observable. */
        fun cancel(): Boolean = cancelAction()
        /** Closing a preparation retires its review even after completion;
         * committed receipt values remain observable. */
        override fun close() {
            cancel()
            result.whenComplete { value, _ -> (value as? AutoCloseable)?.close() }
        }
    }
    inner class Review internal constructor(internal val job: NativeBridge.NativeImport, internal val source: Uri,
        val operationId: UUID, val summary: Summary) : Closeable {
        private var closed = false
        private var claimed = false
        internal fun claim() { synchronized(lock) {
            if (closed || claimed || this !in reviews) throw Failure(FailureReason.InvalidInput, operationId)
            claimed = true
        } }
        fun publish(proof: NativeBridge.Verification, confirmed: Boolean, acknowledgeOmissions: Boolean): Pending<Receipt> =
            publishReview(this, proof, confirmed, acknowledgeOmissions)
        override fun close() {
            val release = synchronized(lock) { if (closed) false else { closed = true; reviews.remove(this); true } }
            // Native close revokes registry/authority only; it runs no provider I/O.
            if (release) job.close()
        }
    }
    private val lock = Any()
    private var closed = false
    private val operations = mutableSetOf<Operation<*>>()
    private val reviews = mutableSetOf<Review>()
    @Volatile private var workerThread: Thread? = null
    private val worker = ThreadPoolExecutor(1, 1, 0, TimeUnit.MILLISECONDS, ArrayBlockingQueue(1),
        { action -> Thread(action, "stem-imports").apply { isDaemon = true; workerThread = this } }, ThreadPoolExecutor.AbortPolicy())
    // At most two admitted operations queue cancellation once each. Admission
    // remains held until their cancellation and worker actually retire.
    private val cancellations = ThreadPoolExecutor(1, 1, 0, TimeUnit.MILLISECONDS, ArrayBlockingQueue(2),
        { action -> Thread(action, "stem-import-cancel").apply { isDaemon = true } }, ThreadPoolExecutor.AbortPolicy())
    private val deadlines = ScheduledThreadPoolExecutor(1) { action -> Thread(action, "stem-import-deadline").apply { isDaemon = true } }
        .apply { removeOnCancelPolicy = true }
    private inner class Operation<T : Any>(val timeoutMillis: Long, val reviewResult: Boolean) {
        val result = CompletableFuture<T>()
        private val state = Any()
        private var cancelled = false
        private var finished = false
        private var retiring = false
        private var resource: AutoCloseable? = null
        private var signal: CancellationSignal? = null
        private var input: InputStream? = null
        private var cancellation: CompletableFuture<Void>? = null
        private val started = System.nanoTime()
        @Volatile var identity: UUID? = null
        @Volatile var attempted = false
        fun remaining(): Long {
            check()
            return (timeoutMillis - TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - started)).coerceAtLeast(1)
        }
        fun check() { synchronized(state) {
            if (cancelled || System.nanoTime() - started >= TimeUnit.MILLISECONDS.toNanos(timeoutMillis))
                throw Failure(FailureReason.Cancelled, identity, attempted)
        } }
        fun attach(value: AutoCloseable) {
            val reject = synchronized(state) { if (cancelled) true else { resource = value; false } }
            if (reject) { value.close(); throw Failure(FailureReason.Cancelled, identity, attempted) }
        }
        fun attach(value: CancellationSignal) {
            val reject = synchronized(state) { if (cancelled) true else { signal = value; false } }
            if (reject) { value.cancel(); throw Failure(FailureReason.Cancelled, identity, attempted) }
        }
        fun attach(value: InputStream) {
            val reject = synchronized(state) { if (cancelled) true else { input = value; false } }
            if (reject) { value.close(); throw Failure(FailureReason.Cancelled, identity, attempted) }
        }
        fun clearInput() { synchronized(state) { input = null; signal = null } }
        fun cancel(): Boolean {
            val values = synchronized(state) {
                if (finished || cancelled) return false
                cancelled = true
                if (retiring) return true
                val done = CompletableFuture<Void>(); cancellation = done
                Triple(resource, signal, input) to done
            }
            cancellations.execute {
                try {
                    try { values.first.first?.close() }
                    finally { try { values.first.second?.cancel() } finally { values.first.third?.close() } }
                } catch (_: Throwable) { /* generic failure; retirement still waits for the worker */ }
                finally { values.second.complete(null) }
            }
            return true
        }
        fun retire() { synchronized(state) { retiring = true; cancellation }?.join() }
        fun complete(value: T?, error: Throwable?) {
            val failure = synchronized(state) {
                finished = true
                resource = null; signal = null; input = null
                if (error == null && !(reviewResult && cancelled)) null
                else if (error is Failure) Failure(error.reason, error.operationId ?: identity, error.publicationMayHaveCommitted || attempted)
                else Failure(if (cancelled || error is CancellationException) FailureReason.Cancelled else FailureReason.Unavailable, identity, attempted)
            }
            if (failure == null) result.complete(requireNotNull(value)) else {
                (value as? Review)?.close()
                result.completeExceptionally(failure)
            }
        }
    }
    private fun observerOnly() {
        if (Thread.currentThread() === workerThread || Looper.myLooper() === Looper.getMainLooper()) throw Failure(FailureReason.Unavailable)
    }
    private fun <T : Any> submit(timeout: Long, reviewResult: Boolean = false, initial: Review? = null,
        cleanup: () -> Unit = {}, action: (Operation<T>) -> T): Pending<T> {
        val operation = Operation<T>(timeout, reviewResult)
        operation.identity = initial?.operationId
        if (initial != null) operation.attach(initial)
        try { synchronized(lock) {
            if (closed) throw Failure(FailureReason.Closed, operation.identity)
            if (operations.size >= 2) throw Failure(FailureReason.Busy, operation.identity)
            operations.add(operation)
            try { worker.execute {
                var value: T? = null; var error: Throwable? = null
                var deadline: java.util.concurrent.ScheduledFuture<*>? = null
                try {
                    operation.check()
                    deadline = deadlines.schedule({ operation.cancel() }, operation.remaining(), TimeUnit.MILLISECONDS)
                    value = action(operation)
                } catch (failure: Throwable) { error = failure }
                finally {
                    deadline?.cancel(false)
                    try { operation.retire(); initial?.close(); cleanup() }
                    catch (failure: Throwable) { if (error == null) error = failure }
                    operation.complete(value, error)
                    synchronized(lock) { operations.remove(operation) }
                }
            } } catch (error: Throwable) { operations.remove(operation); throw error }
        } } catch (error: Throwable) {
            initial?.close(); cleanup()
            throw error as? Failure ?: Failure(FailureReason.Unavailable, operation.identity)
        }
        return Pending(operation.result, operation::cancel, ::observerOnly, { operation.identity }, { operation.attempted })
    }
    fun prepare(document: Uri, credential: CharArray?, recovery: Boolean, limits: Limits): Pending<Review> {
        if (document.scheme != "content" || document.authority.isNullOrEmpty() || document.toString().length > 4096 ||
            recovery && credential == null || credential != null && credential.size !in 1..4096) throw Failure(FailureReason.InvalidInput)
        val secret = credential?.clone()
        return submit(limits.timeoutMillis, reviewResult = true, cleanup = { secret?.fill('\u0000') }) { operation ->
            val host = nativeHandle(); operation.check(); admission()
            val job = NativeBridge.beginNativeImport(host)
            var retained: Review? = null
            try {
                operation.identity = UUID.fromString(job.operation)
                operation.attach(job)
                documents.validate(document); operation.check()
                val signal = CancellationSignal(); operation.attach(signal)
                val bytes = ByteArray(limits.inputBytes)
                try {
                    var count = 0
                    val input = documents.open(document, signal); operation.attach(input)
                    try {
                        while (count < bytes.size) {
                            operation.check()
                            val read = input.read(bytes, count, minOf(32768, bytes.size - count))
                            if (read == -1) break
                            if (read == 0) throw Failure(FailureReason.InvalidInput, operation.identity)
                            count += read
                        }
                        operation.check()
                        if (count == bytes.size && input.read() != -1) throw Failure(FailureReason.InvalidInput, operation.identity)
                    } finally { try { input.close() } finally { operation.clearInput() } }
                    documents.validate(document); operation.check()
                    val exact = bytes.copyOf(count)
                    val row = try { JSONObject(job.prepare(exact, secret, recovery, operation.remaining())) } finally { exact.fill(0) }
                    val review = Review(job, document, requireNotNull(operation.identity), summary(row)); retained = review
                    synchronized(lock) {
                        if (closed) throw Failure(FailureReason.Closed, operation.identity)
                        reviews.add(review)
                    }
                    operation.attach(review); operation.check(); review
                } finally { bytes.fill(0) }
            } catch (error: Throwable) { retained?.close() ?: job.close(); throw error }
        }
    }
    private fun publishReview(review: Review, proof: NativeBridge.Verification, confirmed: Boolean, acknowledge: Boolean): Pending<Receipt> {
        if (!confirmed || review.summary.sourceOmissions.isNotEmpty() && !acknowledge) throw Failure(FailureReason.InvalidInput, review.operationId)
        review.claim()
        return submit(3_600_000, initial = review) { operation ->
            try {
                admission(); documents.validate(review.source); operation.check()
                operation.attempted = true
                // A known committed receipt must survive late cancellation.
                val response = review.job.publish(proof, confirmed, acknowledge)
                fault("publication-response")
                receipt(JSONObject(response), review.operationId)
            } finally { review.close() }
        }
    }
    fun reconcile(operationId: UUID): Pending<Reconciliation> = submit(30_000) { operation ->
        operation.identity = operationId
        val host = nativeHandle(); operation.check()
        val value = NativeBridge.nativeImportReceipt(host, operationId.toString())
        Reconciliation(if (value == "null") null else receipt(JSONObject(value), operationId))
    }
    /** Retire optional input and idle reviews, preserving committed receipts. */
    fun invalidate() {
        val values = synchronized(lock) { operations.toList() to reviews.toList() }
        values.first.forEach { it.cancel() }; values.second.forEach { it.close() }
    }
    override fun close() {
        synchronized(lock) { if (closed) return; closed = true }
        invalidate(); worker.shutdown()
    }
    fun awaitClosed() {
        observerOnly()
        while (!worker.awaitTermination(1, TimeUnit.DAYS)) { }
        deadlines.shutdown()
        while (!deadlines.awaitTermination(1, TimeUnit.DAYS)) { }
        cancellations.shutdown()
        while (!cancellations.awaitTermination(1, TimeUnit.DAYS)) { }
    }
    companion object {
        const val MAX_INPUT_BYTES = 64 * 1024 * 1024 + 208
        private fun summary(row: JSONObject): Summary {
            val omissions = row.getJSONArray("source_omissions")
            val values = (0 until omissions.length()).map { omissions.getString(it) }
            return Summary(row.getInt("source_version"), row.getLong("entities"), row.getLong("blocks"), row.getLong("strokes"),
                row.getLong("history_entries"), row.getLong("protected_objects"), row.getLong("retained_titles"), Collections.unmodifiableList(values))
        }
        private fun receipt(row: JSONObject, expected: UUID): Receipt {
            val ids = row.getJSONArray("entities"); require(ids.length() in 1..10000)
            val entities = (0 until ids.length()).map { UUID.fromString(ids.getString(it)) }
            val operation = UUID.fromString(row.getString("operation")); require(operation == expected)
            val digest = row.getString("artifact_digest"); require(digest.matches(Regex("[0-9a-f]{64}")))
            val completed = row.getLong("completed_ms"); require(completed >= 0 && entities.toSet().size == entities.size)
            return Receipt(operation, digest, Collections.unmodifiableList(entities), completed)
        }
    }
}
