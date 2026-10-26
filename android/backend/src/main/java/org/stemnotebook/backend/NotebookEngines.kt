package org.stemnotebook.backend

import android.os.Looper
import org.json.JSONObject
import java.io.Closeable
import java.util.UUID
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.CancellationException
import java.util.concurrent.CompletableFuture
import java.util.concurrent.ExecutionException
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit

/** One bounded optional worker per backend, independent of authoring. Providers
 * are trusted compiled adapters; this class imports no model and enables none. */
class NotebookEngines internal constructor(private val nativeHandle: () -> Long) : Closeable {
    enum class Task { Maths, Prose, Numeric, Code }
    enum class FailureReason { Unavailable, Cancelled, Busy, Closed, InvalidInput }
    /** Cancellation/error is not a rollback acknowledgement. A native metadata
     * attempt may need fresh version/proposal discovery after an uncertain reply. */
    class Failure(val reason: FailureReason, val metadataMayHaveChanged: Boolean = false) :
        IllegalStateException("Notebook engine operation unavailable")
    data class Staged(val version: UUID)
    data class Installed(val version: UUID)
    /** Read the review through current native authority; retain no transcript here. */
    data class Proposal(val id: UUID, val block: UUID, val revision: Long)

    private val lock = Any()
    private var closed = false
    private val operations = mutableSetOf<Operation<*>>()
    @Volatile private var workerThread: Thread? = null
    private val worker = ThreadPoolExecutor(1, 1, 0, TimeUnit.MILLISECONDS, ArrayBlockingQueue(1),
        { action -> Thread(action, "stem-engines").apply { isDaemon = true; workerThread = this } }, ThreadPoolExecutor.AbortPolicy())

    class Pending<T : Any> internal constructor(private val result: CompletableFuture<T>, private val cancelAction: () -> Boolean,
                                                private val observerCheck: () -> Unit) : Closeable {
        val isReady: Boolean get() = result.isDone
        /** Observing the result does not transfer or repeat the native operation. */
        fun await(): T {
            observerCheck()
            return try { result.get() }
            catch (error: InterruptedException) { cancel(); Thread.currentThread().interrupt(); throw Failure(FailureReason.Cancelled, true) }
            catch (error: ExecutionException) { throw error.cause as? Failure ?: Failure(FailureReason.Unavailable, true) }
        }
        /** Requests cooperative cancellation. Known committed results remain
         * observable, including after close; this never promises no effects. */
        fun cancel(): Boolean = cancelAction()
        override fun close() { cancel() }
    }
    private class Operation<T : Any> {
        val result = CompletableFuture<T>()
        private val lock = Any()
        private var cancelled = false
        private var finished = false
        private var job: NativeBridge.EngineJob? = null
        @Volatile var metadataAttempted = false
        fun check() { synchronized(lock) { if (cancelled) throw Failure(FailureReason.Cancelled, metadataAttempted) } }
        fun attach(value: NativeBridge.EngineJob) {
            val reject = synchronized(lock) { if (cancelled) true else { job = value; false } }
            if (reject) { value.close(); throw Failure(FailureReason.Cancelled, metadataAttempted) }
        }
        fun detach(value: NativeBridge.EngineJob) { synchronized(lock) { if (job === value) job = null } }
        fun cancel(): Boolean {
            val active = synchronized(lock) {
                if (finished || cancelled) return false
                cancelled = true
                job
            }
            active?.close()
            return true
        }
        fun complete(value: T?, error: Throwable?) {
            val failure = synchronized(lock) {
                finished = true
                if (error == null) null else if (error is Failure) error else Failure(
                    if (cancelled || error is CancellationException) FailureReason.Cancelled else FailureReason.Unavailable,
                    metadataAttempted)
            }
            if (failure == null) result.complete(requireNotNull(value)) else result.completeExceptionally(failure)
        }
    }
    private fun observerOnly() {
        if (Thread.currentThread() === workerThread || Looper.myLooper() === Looper.getMainLooper()) throw Failure(FailureReason.Unavailable)
    }
    private fun <T : Any> submit(action: (Operation<T>) -> T): Pending<T> {
        val operation = Operation<T>()
        synchronized(lock) {
            if (closed) throw Failure(FailureReason.Closed)
            if (operations.size >= 2) throw Failure(FailureReason.Busy)
            operations.add(operation)
            try {
                worker.execute {
                    var value: T? = null
                    var failure: Throwable? = null
                    try { operation.check(); value = action(operation) }
                    catch (error: Throwable) { failure = error }
                    finally {
                        operation.complete(value, failure)
                        synchronized(lock) { operations.remove(operation) }
                    }
                }
            } catch (error: Throwable) {
                operations.remove(operation)
                operation.cancel()
                val rejected = error is java.util.concurrent.RejectedExecutionException
                throw Failure(if (rejected) FailureReason.Busy else FailureReason.Unavailable, !rejected)
            }
        }
        return Pending(operation.result, operation::cancel, ::observerOnly)
    }
    /** Explicit consent for a resource-free compiled adapter, not a model-file import. */
    fun stageResourceFree(descriptor: String, confirmed: Boolean): Pending<Staged> {
        if (descriptor.length > 65536 || descriptor.toByteArray(Charsets.UTF_8).size > 65536) throw Failure(FailureReason.InvalidInput)
        return submit { operation ->
            val host = nativeHandle(); operation.check()
            operation.metadataAttempted = true
            val version = JSONObject(NativeBridge.stageResourceFreeEngine(host, descriptor, confirmed))
            Staged(UUID.fromString(version.getString("id")))
        }
    }
    /** maxLoadBytes is an explicit input bound, not a model-memory quota. Budget
     * begins with native admission; uncooperative SDKs still require isolation. */
    fun activate(version: UUID, maxLoadBytes: Long, budgetMillis: Long, factory: NativeBridge.RecogniserFactory): Pending<Installed> {
        budget(budgetMillis)
        if (maxLoadBytes !in 0..Int.MAX_VALUE.toLong()) throw Failure(FailureReason.InvalidInput)
        return submit { operation ->
            val host = nativeHandle(); operation.check()
            val job = NativeBridge.engineActivation(host, version.toString(), maxLoadBytes, budgetMillis)
            try {
                operation.attach(job); operation.metadataAttempted = true
                job.activate(factory)
                Installed(version)
            } finally { operation.detach(job); job.close() }
        }
    }
    fun recognise(block: UUID, region: UUID?, task: Task, engine: String, budgetMillis: Long): Pending<Proposal> {
        budget(budgetMillis)
        if (engine.isEmpty() || engine.length > 256 || engine.toByteArray(Charsets.UTF_8).size > 256) throw Failure(FailureReason.InvalidInput)
        return submit { operation ->
            val host = nativeHandle(); operation.check()
            val job = NativeBridge.engineRecognition(host, block.toString(), region?.toString(), task.name, engine, budgetMillis)
            try {
                operation.attach(job); operation.metadataAttempted = true
                val proposal = JSONObject(job.recognise())
                Proposal(UUID.fromString(proposal.getString("id")), UUID.fromString(proposal.getString("block")), proposal.getLong("revision"))
            } finally { operation.detach(job); job.close() }
        }
    }
    private fun budget(value: Long) { if (value !in 1..3600000) throw Failure(FailureReason.InvalidInput) }
    override fun close() {
        val pending = synchronized(lock) { if (closed) return; closed = true; operations.toList() }
        pending.forEach { it.cancel() }
        worker.shutdown() // queued cancelled requests still retire through their runner
    }
    /** Backend close calls this after native close, retaining its ownership witness. */
    fun awaitClosed() {
        observerOnly()
        while (!worker.awaitTermination(1, TimeUnit.DAYS)) { /* SDK retirement remains owned */ }
    }
}
