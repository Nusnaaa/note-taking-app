package org.stemnotebook.backend

import android.content.Context
import android.content.ContextWrapper
import android.content.Intent
import android.net.Uri
import android.os.CancellationSignal
import org.json.JSONObject
import java.io.Closeable
import java.io.File
import java.io.IOException
import java.io.InputStream
import java.nio.channels.FileChannel
import java.nio.file.Files
import java.nio.file.LinkOption
import java.nio.file.StandardOpenOption
import java.nio.file.attribute.PosixFilePermissions
import java.util.UUID
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.CancellationException
import java.util.concurrent.CompletableFuture
import java.util.concurrent.ExecutionException
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.ScheduledThreadPoolExecutor
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/** Trusted recovered-copy work. One optional worker owns open/decode/review/publish
 * and actual cleanup; no provider, path or credential capability comes from JSON. */
class NotebookRestores internal constructor(
    private val directory: File,
    private val recoveredDirectory: File,
    private val nativeHandle: () -> Long,
    private val access: () -> Unit,
    private val documents: RestoreDocuments,
    private val fault: (String) -> Unit = {},
) : Closeable {
    private val lock = Any()
    private var closed = false
    private var epoch = Any()
    private var review: Review? = null
    private var running: Operation<*>? = null
    private var cancelling = 0
    @Volatile var needsAttention = false
        private set
    private val worker = ThreadPoolExecutor(1, 1, 0, TimeUnit.MILLISECONDS, ArrayBlockingQueue(2),
        { action -> Thread(action, "stem-restore").apply { isDaemon = true } }, ThreadPoolExecutor.AbortPolicy())
    private val cancellations = ThreadPoolExecutor(1, 1, 0, TimeUnit.MILLISECONDS, ArrayBlockingQueue(2),
        { action -> Thread(action, "stem-restore-cancel").apply { isDaemon = true } }, ThreadPoolExecutor.AbortPolicy())
    private val deadlines = ScheduledThreadPoolExecutor(1) { action -> Thread(action, "stem-restore-deadline").apply { isDaemon = true } }
        .apply { removeOnCancelPolicy = true }
    init {
        privateDirectory(directory); privateDirectory(recoveredDirectory)
        needsAttention = Files.newDirectoryStream(directory.toPath()).use { it.iterator().hasNext() }
    }
    /** Caller-selected engineering input/space bounds, not a hard SQLite disk quota
     * or a device performance promise. Peak stage allocation remains native work. */
    data class Limits(val ciphertextBytes: Long, val plaintextBytes: Long, val minimumFreeBytes: Long) {
        init { require(ciphertextBytes > 0 && plaintextBytes > 0 && minimumFreeBytes >= 0) }
    }
    data class Summary(val entities: Long, val blocks: Long, val strokes: Long, val sourceReferences: Long, val sourceAssets: Long, val protectedScopes: Long)
    enum class Outcome { Published, Uncertain }
    enum class FailureReason { AccessChanged, QuotaExceeded, StagingNeedsAttention, InvalidInput, Unavailable }
    class Failure internal constructor(val reason: FailureReason) : IOException("Restore operation unavailable")
    class Pending<T : Any> internal constructor(private val future: CompletableFuture<T>, private val cancelAction: () -> Boolean, private val rejectObservation: () -> Boolean) : Closeable {
        private val taken = AtomicBoolean(false)
        val isReady get() = future.isDone
        fun await(): T {
            check(taken.compareAndSet(false, true)) { "Restore result already claimed" }
            return try { future.get().also { if (rejectObservation()) throw CancellationException("Restore operation cancelled or expired") } }
            catch (error: ExecutionException) { throw error.cause ?: error }
            catch (error: InterruptedException) { cancel(); Thread.currentThread().interrupt(); throw error }
        }
        /** False after publication entry: preserve a possible publication receipt. */
        fun cancel(): Boolean = cancelAction()
        override fun close() { cancel() }
    }
    inner class Review internal constructor(internal val job: NativeBridge.RestoreJob, internal val stage: File,
        internal val source: Uri, internal val stamp: Any, val summary: Summary) : Closeable {
        internal var live = true
        internal var cleanupQueued = false
        fun publish(proof: NativeBridge.Verification, confirmed: Boolean, timeoutMillis: Long = 120_000): Pending<Publication> =
            publishReview(this, proof, confirmed, timeoutMillis)
        override fun close() {
            val active = synchronized(lock) {
                live = false
                queueCleanup(this)
                running?.takeIf { it.job === job }
            }
            active?.cancel(force = true)
        }
    }
    data class Publication(val outcome: Outcome, val library: RecoveredLibrary)
    /** App-private destination capability. Opening is an explicit separate action;
     * a missing/uncertain database never silently becomes an empty library. */
    class RecoveredLibrary internal constructor(private val root: File, val id: UUID, val createdAtMillis: Long, val summary: Summary, val publicationState: Outcome?) {
        val label: String get() = "Recovered library"
        fun open(context: Context): NotebookBackend {
            val database = File(root, "notebook/library.db")
            require(!Files.isSymbolicLink(root.toPath()) && !Files.isSymbolicLink(File(root, "notebook").toPath()))
            require(Files.isRegularFile(database.toPath(), LinkOption.NOFOLLOW_LINKS))
            val marker = File(root, "recovered-library.json")
            require(Files.isRegularFile(marker.toPath(), LinkOption.NOFOLLOW_LINKS) && marker.length() <= 16384)
            val record = readRestoreMarker(marker)
            require(record.getInt("format") == 1 && record.getString("id") == id.toString())
            require(record.getString("state") in setOf("pending", "Published", "Uncertain"))
            // Metadata checks do not open a database descriptor. SQLite itself
            // validates/claims the existing file after LibraryOwnership admission.
            require(database.length() > 0)
            val selected = object : ContextWrapper(context.applicationContext) {
                override fun getNoBackupFilesDir(): File = root
            }
            return NotebookBackend(selected, { platform, native -> PdfWorkspace(platform, native) }, existingLibrary = true)
        }
    }
    private inner class Operation<T : Any>(val timeout: Long) {
        val future = CompletableFuture<T>()
        var cancelled = false
        var publicationEntered = false
        var signal: CancellationSignal? = null
        var job: NativeBridge.RestoreJob? = null
        var cancellation: CompletableFuture<Void>? = null
        var disposal: (() -> Unit)? = null
        var retiring = false
        private val started = System.nanoTime()
        fun check() = synchronized(lock) {
            if (closed || cancelled || System.nanoTime() - started >= TimeUnit.MILLISECONDS.toNanos(timeout))
                throw CancellationException("Restore operation cancelled or expired")
        }
        fun cancel(force: Boolean = false): Boolean {
            val resources = synchronized(lock) {
                if (publicationEntered && !force) return false
                cancelled = true
                disposal?.invoke()
                if (cancellation != null || retiring || running !== this || (job == null && signal == null)) return true
                val done = CompletableFuture<Void>(); cancellation = done; cancelling++
                Triple(job, signal, done)
            }
            cancellations.execute {
                try {
                    try { resources.first?.close() } finally { resources.second?.cancel() }
                } catch (_: Throwable) { needsAttention = true }
                finally { synchronized(lock) { cancelling-- }; resources.third.complete(null) }
            }
            return true
        }
        fun awaitCancellation() { synchronized(lock) { retiring = true; cancellation }?.join() }
    }
    private fun <T : Any> submit(timeout: Long, dispose: (T) -> Unit = {}, retire: () -> Unit = {}, action: (Operation<T>) -> T): Pending<T> {
        require(timeout in 1..3_600_000)
        val operation = Operation<T>(timeout)
        synchronized(lock) {
            check(!closed)
            if (running != null || cancelling != 0) throw RejectedExecutionException("Restore worker occupied")
            running = operation
            try { worker.execute {
                var result: T? = null
                var failure: Throwable? = null
                val timer = deadlines.schedule({ operation.cancel(force = true) }, timeout, TimeUnit.MILLISECONDS)
                try { operation.check(); result = action(operation) }
                catch (error: Throwable) { failure = error }
                finally {
                    timer.cancel(false)
                    operation.awaitCancellation()
                    try { retire() } catch (_: Throwable) { needsAttention = true; if (failure == null) failure = Failure(FailureReason.Unavailable) }
                }
                synchronized(lock) {
                    if (!operation.publicationEntered && failure == null) try { operation.check() } catch (error: Throwable) { failure = error }
                    if (running === operation) running = null
                    if (failure == null) {
                        val value = checkNotNull(result)
                        operation.disposal = { dispose(value) }
                        operation.future.complete(value)
                    } else {
                        result?.let(dispose)
                        var problem = failure
                        repeat(16) { if (problem is java.util.concurrent.CompletionException || problem is ExecutionException) problem = problem?.cause ?: problem }
                        operation.future.completeExceptionally(if (operation.cancelled || problem is CancellationException || problem is android.os.OperationCanceledException)
                            CancellationException("Restore operation cancelled or expired") else (problem as? Failure ?: Failure(FailureReason.Unavailable)))
                    }
                }
            } } catch (error: RejectedExecutionException) { running = null; throw error }
        }
        return Pending(operation.future, { operation.cancel() }, { synchronized(lock) { operation.cancelled && !operation.publicationEntered } })
    }
    private inner class OwnedInput(private val input: InputStream, private val source: Uri,
        private val operation: Operation<*>, private val stage: File, private val limits: Limits) : NativeBridge.RestoreInput {
        private val retired = AtomicBoolean(false)
        var failure: FailureReason? = null
        private var count = 0L
        override fun read(ciphertext: ByteArray): Int {
            operation.check(); documents.validate(source); checkSpace(stage, limits)
            val read = input.read(ciphertext)
            if (read == 0 || read < -1 || read > ciphertext.size) { failure = FailureReason.InvalidInput; throw Failure(FailureReason.InvalidInput) }
            if (read > 0) {
                if (read.toLong() > limits.ciphertextBytes - count) { failure = FailureReason.QuotaExceeded; throw Failure(FailureReason.QuotaExceeded) }
                count += read
            }
            operation.check(); documents.validate(source)
            return read
        }
        override fun retire() {
            if (!retired.compareAndSet(false, true)) return
            try { fault("input-retire") } finally { input.close() }
        }
    }
    /** flags are from a deliberate read picker; no persistable grant is taken. */
    fun prepare(document: Uri, pickerFlags: Int, credential: CharArray, recoveryKey: Boolean, limits: Limits,
        timeoutMillis: Long = 120_000): Pending<Review> {
        require(document.scheme == "content" && document.toString().length <= 4096 && pickerFlags and Intent.FLAG_GRANT_READ_URI_PERMISSION != 0)
        require(credential.size <= 1024)
        synchronized(lock) { check(!closed && review == null) { "Restore review still owned" } }
        val secret = credential.clone()
        return try {
            submit(timeoutMillis, { it.close() }, { secret.fill('\u0000') }) { operation ->
                var stage: File? = null
                var job: NativeBridge.RestoreJob? = null
                var input: OwnedInput? = null
                try {
                    synchronized(lock) { check(review == null) }
                    access(); operation.check()
                    if (Files.newDirectoryStream(directory.toPath()).use { it.iterator().hasNext() }) { needsAttention = true; throw Failure(FailureReason.StagingNeedsAttention) }
                    val stamp = synchronized(lock) { epoch }
                    val stageFile = File(directory, UUID.randomUUID().toString()); stage = stageFile; privateDirectory(stageFile)
                    checkSpace(stageFile, limits)
                    val nativeJob = NativeBridge.beginRestore(nativeHandle(), timeoutMillis); job = nativeJob
                    synchronized(lock) { operation.check(); operation.job = nativeJob; operation.signal = CancellationSignal() }
                    documents.validate(document)
                    val stream = documents.open(document, checkNotNull(operation.signal))
                    input = OwnedInput(stream, document, operation, stageFile, limits)
                    operation.check(); fault("before-native")
                    val nativeSummary = nativeJob.prepare(secret, recoveryKey, input, stageFile.absolutePath, limits.plaintextBytes)
                    fault("after-native"); operation.check(); access(); documents.validate(document); checkSpace(stageFile, limits)
                    val data = JSONObject(nativeSummary)
                    val summary = decodeSummary(data)
                    synchronized(lock) {
                        operation.check()
                        if (epoch !== stamp) throw Failure(FailureReason.AccessChanged)
                        Review(nativeJob, stageFile, document, stamp, summary).also { review = it; job = null; stage = null }
                    }
                } catch (error: Throwable) { throw input?.failure?.let { Failure(it) } ?: error }
                finally {
                    secret.fill('\u0000')
                    try { input?.retire() } finally {
                        operation.awaitCancellation()
                        try { job?.close() } finally {
                            stage?.let(::removeStage)
                            synchronized(lock) { operation.job = null; operation.signal = null }
                        }
                    }
                }
            }
        } catch (error: Throwable) { secret.fill('\u0000'); throw error }
    }
    private fun publishReview(selected: Review, proof: NativeBridge.Verification, confirmed: Boolean, timeout: Long): Pending<Publication> {
        require(confirmed)
        synchronized(lock) { check(!closed && review === selected && selected.live) }
        return submit(timeout) { operation ->
            var root: File? = null
            try {
                synchronized(lock) {
                    operation.check(); check(selected.live && review === selected)
                    selected.cleanupQueued = true; operation.job = selected.job
                }
                operation.check(); access(); documents.validate(selected.source)
                val id = UUID.randomUUID()
                val created = System.currentTimeMillis()
                root = File(recoveredDirectory, id.toString())
                Files.createDirectory(root.toPath(), PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rwx------")))
                privateDirectory(File(root, "notebook"))
                val marker = JSONObject().put("format", 1).put("id", id.toString()).put("created_at_millis", created)
                    .put("label", "Recovered library").put("state", "pending").put("summary", encodeSummary(selected.summary))
                val manifest = File(root, "recovered-library.json")
                Files.write(manifest.toPath(), marker.toString().toByteArray(Charsets.UTF_8), StandardOpenOption.CREATE_NEW, StandardOpenOption.WRITE)
                FileChannel.open(manifest.toPath(), StandardOpenOption.WRITE).use { it.force(true) }; syncDirectory(root); syncDirectory(recoveredDirectory)
                fault("before-publish")
                synchronized(lock) {
                    operation.check()
                    check(selected.live && review === selected)
                    if (epoch !== selected.stamp) throw Failure(FailureReason.AccessChanged)
                    selected.live = false
                    operation.job = selected.job
                    operation.publicationEntered = true
                }
                var outcome = try {
                    val freshProof = NativeBridge.Verification { challenge ->
                        try {
                            operation.check(); documents.validate(selected.source)
                            val verified = proof.verify(challenge)
                            operation.check(); documents.validate(selected.source)
                            synchronized(lock) { check(epoch === selected.stamp) }
                            verified
                        } catch (_: Throwable) { false }
                    }
                    val value = selected.job.publish(File(root, "notebook/library.db").absolutePath, freshProof, true)
                    fault("after-publish")
                    if (value == "\"Published\"") Outcome.Published else Outcome.Uncertain
                } catch (_: Throwable) { Outcome.Uncertain }
                try {
                    marker.put("state", outcome.name)
                    val update = File(root, "recovered-library.update")
                    Files.write(update.toPath(), marker.toString().toByteArray(Charsets.UTF_8), StandardOpenOption.CREATE_NEW, StandardOpenOption.WRITE)
                    FileChannel.open(update.toPath(), StandardOpenOption.WRITE).use { it.force(true) }
                    Files.move(update.toPath(), manifest.toPath(), java.nio.file.StandardCopyOption.ATOMIC_MOVE, java.nio.file.StandardCopyOption.REPLACE_EXISTING)
                    syncDirectory(root)
                } catch (_: Exception) { outcome = Outcome.Uncertain }
                if (outcome == Outcome.Uncertain) needsAttention = true
                Publication(outcome, RecoveredLibrary(root, id, created, selected.summary, outcome))
            } finally {
                operation.awaitCancellation()
                try { selected.job.close() } catch (_: Throwable) { needsAttention = true }
                removeStage(selected.stage)
                synchronized(lock) { selected.live = false; if (review === selected) review = null; operation.job = null }
                if (!operation.publicationEntered) root?.let { candidate ->
                    File(candidate, "recovered-library.json").delete(); File(candidate, "notebook").delete(); if (!candidate.delete()) needsAttention = true
                }
            }
        }
    }
    data class CataloguePage(val libraries: List<RecoveredLibrary>, val next: UUID?)
    /** Persistent recovered-copy discovery. Counts describe the restored snapshot,
     * not edits subsequently made in an explicitly opened recovered library. */
    fun recoveredLibraries(after: UUID? = null, limit: Int = 32, timeoutMillis: Long = 120_000): Pending<CataloguePage> {
        require(limit in 1..100)
        return submit(timeoutMillis) { operation ->
            access(); operation.check()
            val candidates = java.util.TreeMap<UUID, File>()
            Files.newDirectoryStream(recoveredDirectory.toPath()).use { entries ->
                for (path in entries) {
                    operation.check()
                    val id = try { UUID.fromString(path.fileName.toString()) } catch (_: Exception) { needsAttention = true; continue }
                    if (id.toString() != path.fileName.toString() || (after != null && id <= after)) continue
                    candidates[id] = path.toFile()
                    if (candidates.size > limit + 1) candidates.pollLastEntry()
                }
            }
            val records = mutableListOf<RecoveredLibrary>()
            var last: UUID? = null
            for ((id, root) in candidates.entries.take(limit)) {
                operation.check(); last = id
                try {
                    require(Files.isDirectory(root.toPath(), LinkOption.NOFOLLOW_LINKS))
                    val marker = File(root, "recovered-library.json")
                    require(Files.isRegularFile(marker.toPath(), LinkOption.NOFOLLOW_LINKS) && marker.length() <= 16384)
                    val data = readRestoreMarker(marker)
                    require(data.getInt("format") == 1 && data.getString("id") == id.toString())
                    val state = data.getString("state")
                    require(state in setOf("pending", "Published", "Uncertain"))
                    val outcome = if (state == "pending") null else Outcome.valueOf(state)
                    if (outcome != Outcome.Published) needsAttention = true
                    records += RecoveredLibrary(root, id, data.getLong("created_at_millis"), decodeSummary(data.getJSONObject("summary")), outcome)
                } catch (_: Exception) { needsAttention = true }
            }
            CataloguePage(records.toList(), if (candidates.size > limit) last else null)
        }
    }
    private fun decodeSummary(data: JSONObject): Summary {
        val values = listOf("entities", "blocks", "strokes", "source_references", "source_assets", "protected_scopes")
            .map { data.getLong(it).also { n -> require(n >= 0) } }
        return Summary(values[0], values[1], values[2], values[3], values[4], values[5])
    }
    private fun encodeSummary(summary: Summary): JSONObject = JSONObject().put("entities", summary.entities).put("blocks", summary.blocks)
        .put("strokes", summary.strokes).put("source_references", summary.sourceReferences).put("source_assets", summary.sourceAssets).put("protected_scopes", summary.protectedScopes)
    private fun queueCleanup(selected: Review) {
        if (selected.cleanupQueued) return
        selected.cleanupQueued = true
        worker.execute {
            try { selected.job.close(); removeStage(selected.stage) }
            catch (_: Throwable) { needsAttention = true }
            finally { synchronized(lock) { if (review === selected) review = null } }
        }
    }
    fun invalidate() {
        val active = synchronized(lock) {
            if (closed) return
            epoch = Any(); review?.let { it.live = false; queueCleanup(it) }
            running
        }
        active?.cancel(force = true)
    }
    override fun close() {
        val active = synchronized(lock) {
            if (closed) return
            closed = true; epoch = Any(); review?.let { it.live = false; queueCleanup(it) }
            running
        }
        active?.cancel(force = true)
        worker.shutdown()
    }
    internal fun awaitClosed() {
        awaitStopped(worker)
        cancellations.shutdown(); deadlines.shutdownNow()
        awaitStopped(cancellations, deadlines)
    }
    private fun removeStage(stage: File) {
        // Delete only an empty, exactly owned directory. Unknown/orphan bytes stay.
        try { fault("stage-cleanup"); Files.deleteIfExists(stage.toPath()) }
        catch (_: Exception) { needsAttention = true }
    }
    private fun checkSpace(stage: File, limits: Limits) {
        if (Files.getFileStore(stage.toPath()).usableSpace < limits.minimumFreeBytes) throw Failure(FailureReason.QuotaExceeded)
    }
    private fun privateDirectory(file: File) {
        val path = file.toPath()
        if (!Files.exists(path, LinkOption.NOFOLLOW_LINKS)) Files.createDirectory(path, PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rwx------")))
        require(Files.isDirectory(path, LinkOption.NOFOLLOW_LINKS) && !Files.isSymbolicLink(path))
        require(Files.getPosixFilePermissions(path).none { it.name.startsWith("GROUP_") || it.name.startsWith("OTHERS_") })
    }
    private fun syncDirectory(file: File) { FileChannel.open(file.toPath(), StandardOpenOption.READ).use { it.force(true) } }
}

/** Bounded read even if an app-private marker changes while it is being read. */
private fun readRestoreMarker(file: File): JSONObject {
    require(Files.isRegularFile(file.toPath(), LinkOption.NOFOLLOW_LINKS))
    val bytes = ByteArray(16385)
    var count = 0
    Files.newInputStream(file.toPath(), StandardOpenOption.READ, LinkOption.NOFOLLOW_LINKS).use { input ->
        while (count < bytes.size) {
            val read = input.read(bytes, count, bytes.size - count)
            if (read < 0) break
            require(read > 0); count += read
        }
    }
    require(count <= 16384)
    return JSONObject(String(bytes, 0, count, Charsets.UTF_8))
}
