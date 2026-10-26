package org.stemnotebook.backend

import android.os.Looper
import org.json.JSONObject
import java.io.Closeable
import java.io.File
import java.util.UUID
import java.util.Collections
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.CancellationException
import java.util.concurrent.CompletableFuture
import java.util.concurrent.ExecutionException
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit

/** One bounded optional worker per backend, independent of authoring. Providers
 * are trusted compiled adapters. Construction imports/enables no model. */
class NotebookEngines internal constructor(private val modelRoot: File? = null, private val nativeHandle: () -> Long) : Closeable {
    enum class Task { Maths, Prose, Numeric, Code }
    enum class FailureReason { Unavailable, Cancelled, Busy, Closed, InvalidInput }
    /** Cancellation/error is not a rollback acknowledgement. A native metadata
     * attempt may need fresh version/proposal discovery after an uncertain reply. */
    class Failure(val reason: FailureReason, val metadataMayHaveChanged: Boolean = false) :
        IllegalStateException("Notebook engine operation unavailable")
    enum class Installation { Staged, Installed, Removed }
    /** Descriptor JSON preserves provenance; managed resource paths are omitted. */
    data class Version(val id: UUID, val engine: String, val version: String, val descriptorJson: String,
                       val installation: Installation, val bytes: Long, val installedMillis: Long?,
                       val active: Boolean, val runtimeReady: Boolean)
    enum class RecoveryState { Interrupted, RecordedComplete, ReviewRequired, Invalid }
    data class RecoveryEntry(val id: UUID, val state: RecoveryState, val descriptorJson: String?,
                             val totalBytes: Long?, val committedBytes: Long?, val consentMillis: Long?)
    data class RecoveryPage(val entries: List<RecoveryEntry>, val next: UUID?)
    data class Selected(val task: Task, val engine: String)
    data class Default(val task: Task, val engine: String, val runtimeReady: Boolean)
    data class Catalogue(val versions: List<Version>, val next: UUID?, val defaults: List<Default>)
    data class Staged(val version: UUID)
    data class Installed(val version: UUID)
    /** Logical disablement only; files and independently pinned SDK resources remain retained. */
    data class Removed(val version: UUID)
    data class Transfer(val version: UUID, val committedBytes: Long)
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
        private var job: AutoCloseable? = null
        @Volatile var metadataAttempted = false
        fun check() { synchronized(lock) { if (cancelled) throw Failure(FailureReason.Cancelled, metadataAttempted) } }
        fun attach(value: AutoCloseable) {
            val reject = synchronized(lock) { if (cancelled) true else { job = value; false } }
            if (reject) { value.close(); throw Failure(FailureReason.Cancelled, metadataAttempted) }
        }
        fun detach(value: AutoCloseable) { synchronized(lock) { if (job === value) job = null } }
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
    /** Explicit version removal. This does not acknowledge disk reclamation or
     * actual retirement of every previously admitted inference worker. */
    fun remove(version: UUID, budgetMillis: Long): Pending<Removed> {
        budget(budgetMillis)
        return submit { operation ->
            val host = nativeHandle(); operation.check()
            val job = NativeBridge.engineRemoval(host,version.toString(),budgetMillis)
            try {
                operation.attach(job);operation.metadataAttempted = true
                job.remove();Removed(version)
            } finally {operation.detach(job);job.close()}
        }
    }
    /** Read-only private-depot discovery. RecordedComplete is not current hash
     * verification or runtime readiness; resume/finish and activation are separate. */
    fun recover(after: UUID? = null, limit: Int = 32, budgetMillis: Long = 30000): Pending<RecoveryPage> {
        budget(budgetMillis)
        if (limit !in 1..128) throw Failure(FailureReason.InvalidInput)
        val root = modelRoot ?: throw Failure(FailureReason.Unavailable)
        return submit { operation ->
            val host = nativeHandle(); operation.check()
            val job = NativeBridge.engineRecovery(host,budgetMillis)
            try {
                operation.attach(job)
                val page = JSONObject(job.scan(root.absolutePath,after?.toString(),limit))
                val rows = page.getJSONArray("entries")
                val entries = ArrayList<RecoveryEntry>()
                for (index in 0 until rows.length()) {
                    val row = rows.getJSONObject(index)
                    entries.add(RecoveryEntry(UUID.fromString(row.getString("id")),RecoveryState.valueOf(row.getString("state")),
                        if(row.isNull("descriptor")) null else row.getJSONObject("descriptor").toString(),
                        if(row.isNull("total")) null else row.getLong("total"),
                        if(row.isNull("committed")) null else row.getLong("committed"),
                        if(row.isNull("consent_ms")) null else row.getLong("consent_ms")))
                }
                RecoveryPage(Collections.unmodifiableList(entries),if(page.isNull("next")) null else UUID.fromString(page.getString("next")))
            } finally { operation.detach(job);job.close() }
        }
    }
    /** Bounded metadata snapshot through current native access. No model load. */
    fun catalogue(after: UUID? = null, limit: Int = 32): Pending<Catalogue> {
        if (limit !in 1..128) throw Failure(FailureReason.InvalidInput)
        return submit { operation ->
            val host = nativeHandle(); operation.check()
            val page = JSONObject(NativeBridge.engineCatalogue(host, after?.toString(), limit))
            val rows = page.getJSONArray("versions")
            val versions = ArrayList<Version>()
            for (index in 0 until rows.length()) {
                val row = rows.getJSONObject(index)
                val descriptor = row.getJSONObject("descriptor")
                versions.add(Version(UUID.fromString(row.getString("id")), descriptor.getString("id"),
                    descriptor.getString("version"), descriptor.toString(), Installation.valueOf(row.getString("state")),
                    row.getLong("bytes"), if (row.isNull("installed_ms")) null else row.getLong("installed_ms"),
                    row.getBoolean("active"), row.getBoolean("runtime_ready")))
            }
            val preferences = page.getJSONArray("defaults")
            val defaults = ArrayList<Default>()
            for (index in 0 until preferences.length()) {
                val row = preferences.getJSONObject(index)
                defaults.add(Default(Task.valueOf(row.getString("task")), row.getString("engine"), row.getBoolean("runtime_ready")))
            }
            Catalogue(Collections.unmodifiableList(versions), if (page.isNull("next")) null else UUID.fromString(page.getString("next")),
                Collections.unmodifiableList(defaults))
        }
    }
    fun selectDefault(task: Task, engine: String): Pending<Selected> {
        if (engine.isEmpty() || engine.length > 256 || engine.toByteArray(Charsets.UTF_8).size > 256) throw Failure(FailureReason.InvalidInput)
        return submit { operation ->
            val host = nativeHandle(); operation.check(); operation.metadataAttempted = true
            NativeBridge.selectEngineDefault(host, task.name, engine)
            // This acknowledgement does not promise readiness at a future use.
            Selected(task, engine)
        }
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
    fun recognise(block: UUID, region: UUID?, task: Task, engine: String?, budgetMillis: Long): Pending<Proposal> {
        budget(budgetMillis)
        if (engine != null && (engine.isEmpty() || engine.length > 256 || engine.toByteArray(Charsets.UTF_8).size > 256)) throw Failure(FailureReason.InvalidInput)
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
    /** Deliberate model-file copy into the private no-backup depot. started runs
     * before source opens so callers can retain the stable transfer ID/offset.
     * Failures preserve resumable files; source factories clean partial opens. */
    fun importResource(descriptor: String, total: Long, confirmed: Boolean, resume: UUID?, budgetMillis: Long,
                       started: (Transfer) -> Unit = {},
                       input: (Transfer, NativeBridge.EngineControl) -> NativeBridge.EngineInput): Pending<Staged> {
        budget(budgetMillis)
        val root = modelRoot ?: throw Failure(FailureReason.Unavailable)
        if (total !in 1..16L*1024*1024*1024 || descriptor.length > 60*1024 || descriptor.toByteArray(Charsets.UTF_8).size > 60*1024) throw Failure(FailureReason.InvalidInput)
        return submit { operation ->
            val host = nativeHandle(); operation.check()
            if (java.nio.file.Files.isSymbolicLink(root.toPath())) throw Failure(FailureReason.Unavailable)
            operation.metadataAttempted = true // private ledger/files may be published before a reply
            val job = NativeBridge.engineImport(host,descriptor,total,confirmed,root.absolutePath,resume?.toString(),budgetMillis)
            try {
                operation.attach(job)
                val receipt = Transfer(UUID.fromString(job.version),job.committedBytes)
                started(receipt);operation.check()
                val version = JSONObject(job.copy { control -> input(receipt,control) })
                Staged(UUID.fromString(version.getString("id")))
            } finally { operation.detach(job);job.close() }
        }
    }
    fun importResource(descriptor: String, total: Long, confirmed: Boolean, resume: UUID?, budgetMillis: Long,
                       source: EngineModelSource, started: (Transfer) -> Unit = {}): Pending<Staged> =
        importResource(descriptor,total,confirmed,resume,budgetMillis,started) { transfer, control -> source.open(transfer.committedBytes,control) }
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
