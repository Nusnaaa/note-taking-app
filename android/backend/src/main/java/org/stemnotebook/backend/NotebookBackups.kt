package org.stemnotebook.backend

import android.content.Intent
import android.net.Uri
import android.os.CancellationSignal
import android.os.ParcelFileDescriptor
import android.provider.DocumentsContract
import java.io.Closeable
import java.io.File
import java.io.IOException
import java.nio.ByteBuffer
import java.nio.channels.FileChannel
import java.nio.file.Files
import java.nio.file.LinkOption
import java.nio.file.StandardOpenOption
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.CancellationException
import java.util.concurrent.CompletableFuture
import java.util.concurrent.ExecutionException
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit

/** Trusted application backup workflow. No path/credential capability comes from
 * notebook JSON. One review/artifact and one optional worker bound retained work.
 * Pending.await belongs on an observer thread, never the authoring/input thread. */
class NotebookBackups internal constructor(
    private val directory: File,
    private val privateRoot: File,
    private val nativeHandle: () -> Long,
    private val barrier: (Any?) -> Any,
    private val current: (Any) -> Boolean,
    private val fault: (String) -> Unit = {},
    private val documents: BackupDocuments,
) : Closeable {
    private val lock = Any()
    private var closed = false
    private var review: Review? = null
    private var prepared: Prepared? = null
    private var running: Operation<*>? = null
    private var destination: ProviderDestination? = null
    private var cancelling = false
    @Volatile var needsAttention: Boolean = false
        private set
    private val worker = ThreadPoolExecutor(1, 1, 0, TimeUnit.MILLISECONDS, ArrayBlockingQueue(2),
        { action -> Thread(action, "stem-backup").apply { isDaemon = true } }, ThreadPoolExecutor.AbortPolicy())
    private val cancellations = ThreadPoolExecutor(1, 1, 0, TimeUnit.MILLISECONDS, ArrayBlockingQueue(1),
        { action -> Thread(action, "stem-backup-cancel").apply { isDaemon = true } }, ThreadPoolExecutor.AbortPolicy())
    private val deadlines = java.util.concurrent.ScheduledThreadPoolExecutor(1) { action ->
        Thread(action, "stem-backup-deadline").apply { isDaemon = true }
    }.apply { removeOnCancelPolicy = true }
    init {
        check(directory.isDirectory || directory.mkdirs())
        require(!Files.isSymbolicLink(directory.toPath()))
    }
    class Pending<T : Any> internal constructor(private val future: CompletableFuture<T>, private val cancelAction: () -> Boolean, private val partialOutcome: () -> Boolean) : Closeable {
        private val cancelled = java.util.concurrent.atomic.AtomicBoolean(false)
        private val taken = java.util.concurrent.atomic.AtomicBoolean(false)
        val isReady: Boolean get() = future.isDone
        fun await(): T {
            check(taken.compareAndSet(false, true)) { "Backup result already claimed" }
            if (cancelled.get() && !partialOutcome()) throw CancellationException("Backup operation cancelled")
            return try { future.get().also { if (cancelled.get() && !partialOutcome()) throw CancellationException("Backup operation cancelled") } }
            catch (error: ExecutionException) { throw error.cause ?: error }
            catch (error: InterruptedException) { cancel(); Thread.currentThread().interrupt(); throw error }
        }
        /** False once publication has entered its non-reversible commit phase. */
        fun cancel(): Boolean { val applied = cancelAction(); if (applied) cancelled.set(true); return applied }
        override fun close() { cancel() }
    }
    inner class Review internal constructor(internal val native: NativeBridge.BackupReview, internal val epoch: Any) : Closeable {
        internal var live = true
        val includesOriginalInk: Boolean get() = native.includesOriginalInk
        val sourceAssets: Long get() = native.sourceAssets
        val omissions: List<String> get() = native.omissions().toList()
        override fun close() {
            val release = synchronized(lock) {
                if (!live) false else { live = false; if (review === this) review = null; true }
            }
            if (release) try { native.close() } catch (_: Exception) { }
        }
    }
    inner class Prepared internal constructor(internal val file: File, private val key: CharArray,
        internal val epoch: Any, val bytes: Long, val includesOriginalInk: Boolean,
        val sourceAssets: Long, val omissions: List<String>, internal val digest: ByteArray) : Closeable {
        internal var live = true
        internal var cleanupQueued = false
        /** Explicit trusted recovery workflow; caller owns and clears this copy. */
        fun copyRecoveryKey(): CharArray = synchronized(lock) {
            check(!closed && live && prepared === this); key.clone()
        }
        fun publish(destination: File, confirmed: Boolean, timeoutMillis: Long = 120_000): Pending<Publication> =
            publishPrepared(this, destination, confirmed, timeoutMillis)
        fun reviewProvider(tree: Uri, pickerFlags: Int, timeoutMillis: Long = 120_000): Pending<ProviderDestination> =
            reviewProviderDestination(this, tree, pickerFlags, timeoutMillis)
        override fun close() = synchronized(lock) {
            destination?.let { it.live = false }; destination = null
            live = false; key.fill('\u0000'); queueCleanup(this)
        }
    }
    inner class ProviderDestination internal constructor(internal val artifact: Prepared, internal val tree: Uri,
        internal val observed: BackupDocuments.Selection) : Closeable {
        internal var live = true
        val label: String get() = synchronized(lock) { check(live && !closed); observed.label }
        fun publish(confirmed: Boolean, timeoutMillis: Long = 120_000): Pending<ProviderPublication> =
            publishProvider(this, confirmed, timeoutMillis)
        override fun close() = synchronized(lock) {
            live = false; if (destination === this) destination = null
        }
    }
    /** Only ciphertext and an intentionally created document URI cross the provider
     * boundary. No key/password, raw notebook path or cloud-sync claim is included. */
    class ProviderPublication internal constructor(val document: Uri?, val bytes: Long, val outcome: Outcome,
        val descriptorSyncAcknowledged: Boolean, val coversCurrentAuthoring: Boolean)
    /** Published is a durable local-filesystem acknowledgement, not cloud sync.
     * Uncertain means a final encrypted file may exist: inspect before retrying.
     * No destination path, key or notebook content is present in this result. */
    data class Publication(val bytes: Long, val outcome: Outcome, val coversCurrentAuthoring: Boolean)
    enum class Outcome { Published, ProviderVerified, Uncertain }
    enum class FailureReason { RecoveryRequired, SourceChanged, QuotaExceeded, StagingNeedsAttention, IntegrityFailed, DestinationExists, Unavailable }
    class Failure internal constructor(val reason: FailureReason) : IOException("Backup operation unavailable")

    internal inner class Operation<T : Any>(val timeout: Long) {
        val future = CompletableFuture<T>()
        private val started = System.nanoTime()
        var cancelled = false
        var committing = false
        @Volatile var partialOutcome = false
        var signal: CancellationSignal? = null
        var signalCancelled = false
        var nativeJob: NativeBridge.BackupJob? = null
        var disposal: (() -> Unit)? = null
        fun check() = synchronized(lock) {
            if (!committing && (closed || cancelled || System.nanoTime() - started >= TimeUnit.MILLISECONDS.toNanos(timeout)))
                throw CancellationException("Backup operation cancelled or expired")
        }
        fun remainingNanos(): Long = maxOf(0, TimeUnit.MILLISECONDS.toNanos(timeout) - (System.nanoTime() - started))
        fun completionCheck() { if (!partialOutcome) check() }
        fun providerSignal(): CancellationSignal = synchronized(lock) {
            check(); CancellationSignal().also { signal = it }
        }
        fun cancel(): Boolean {
            val pair = synchronized(lock) { if (committing) return false
                cancelled = true
                val active = signal
                if (active != null && !signalCancelled) {
                    signalCancelled = true; cancelling = true
                    cancellations.execute { try { active.cancel() } finally { synchronized(lock) { cancelling = false } } }
                }
                nativeJob to disposal }
            try { pair.first?.close() } catch (_: Exception) { }
            pair.second?.invoke()
            return true
        }
    }
    private fun <T : Any> submit(timeout: Long, dispose: (T) -> Unit, retire: () -> Unit = {}, action: (Operation<T>) -> T): Pending<T> {
        require(timeout in 1..3_600_000)
        val operation = Operation<T>(timeout)
        synchronized(lock) {
            check(!closed)
            if (running != null || cancelling) throw RejectedExecutionException("Backup worker occupied")
            running = operation
            try {
                worker.execute {
                    var value: T? = null
                    var failure: Throwable? = null
                    try { operation.check(); value = action(operation); operation.completionCheck() }
                    catch (error: Throwable) { failure = error }
                    try { retire() } catch (error: Throwable) { if (failure == null) failure = error }
                    synchronized(lock) {
                        if (failure == null) try { operation.completionCheck() } catch (error: Throwable) { failure = error }
                        operation.signal = null
                        if (running === operation) running = null
                        var problem = failure
                        var wrappers = 0
                        while ((problem is java.util.concurrent.CompletionException || problem is ExecutionException) && wrappers++ < 16) {
                            problem = problem?.cause ?: break
                        }
                        if (failure == null) {
                            val result = checkNotNull(value)
                            operation.disposal = { dispose(result) }
                            operation.future.complete(result)
                        } else {
                            value?.let { dispose(it) }
                            operation.future.completeExceptionally(if (problem is CancellationException || problem is android.os.OperationCanceledException || operation.cancelled)
                                CancellationException("Backup operation cancelled or expired") else (problem as? Failure ?: Failure(FailureReason.Unavailable)))
                        }
                    }
                }
            } catch (error: RejectedExecutionException) { running = null; throw error }
        }
        return Pending(operation.future, { operation.cancel() }, { operation.partialOutcome })
    }
    fun review(originalInk: Boolean = true, sourceAssets: Boolean = false, timeoutMillis: Long = 120_000): Pending<Review> {
        synchronized(lock) { check(review == null && prepared == null) { "Backup review or artifact still owned" } }
        return submit(timeoutMillis, { it.close() }) { operation ->
            synchronized(lock) { check(review == null && prepared == null) }
            val epoch = barrier(null)
            operation.check()
            val native = NativeBridge.reviewBackup(nativeHandle(), originalInk, sourceAssets)
            var owned = true
            try {
                barrier(epoch); operation.check()
                synchronized(lock) {
                    operation.check()
                    Review(native, epoch).also { review = it; owned = false }
                }
            } finally { if (owned) native.close() }
        }
    }
    /** Creates only an encrypted, private, synced artifact. Publication is a
     * separate deliberate operation; this is not a Backed up acknowledgement. */
    fun create(review: Review, password: CharArray, proof: NativeBridge.Verification, confirmed: Boolean,
        maximumBytes: Long, timeoutMillis: Long = 120_000): Pending<Prepared> {
        require(password.size <= 1024 && maximumBytes > 0)
        synchronized(lock) { check(review === this.review && review.live && prepared == null) }
        val secret = password.clone()
        return try {
            submit(timeoutMillis, { it.close() }, { secret.fill('\u0000'); review.close(); try { review.native.close() } catch (_: Exception) { } }) { operation ->
                var file: File? = null
                var key: CharArray? = null
                try {
                    barrier(review.epoch); operation.check()
                    synchronized(lock) {
                        check(review === this.review && review.live)
                        review.live = false; this.review = null
                    }
                    val job = review.native.begin(timeoutMillis)
                    synchronized(lock) { operation.nativeJob = job }
                    job.use {
                        operation.check()
                        // Retain orphan/unknown entries for explicit recovery; never
                        // sweep another instance's or unclassified private data.
                        if (Files.newDirectoryStream(directory.toPath()).use { it.iterator().hasNext() }) { needsAttention = true; throw Failure(FailureReason.StagingNeedsAttention) }
                        file = Files.createTempFile(directory.toPath(), "backup-", ".pending").toFile()
                        val output = FileChannel.open(file!!.toPath(), StandardOpenOption.WRITE, LinkOption.NOFOLLOW_LINKS)
                        val sink = object : NativeBridge.BackupOutput {
                            var count = 0L
                            val digest = java.security.MessageDigest.getInstance("SHA-256")
                            var failure: FailureReason? = null
                            override fun write(ciphertext: ByteArray) {
                                operation.check()
                                if (ciphertext.size.toLong() > maximumBytes - count) { failure = FailureReason.QuotaExceeded; throw IOException("Backup quota exceeded") }
                                val buffer = ByteBuffer.wrap(ciphertext)
                                while (buffer.hasRemaining()) { operation.check(); output.write(buffer) }
                                count += ciphertext.size; digest.update(ciphertext)
                            }
                            override fun flush() { operation.check() }
                            override fun retire() { try { fault("stage-retire"); output.force(true) } finally { output.close() } }
                        }
                        // The Java wrapper owns sink retirement once run is called.
                        // Retire explicitly if an earlier trusted test/platform hook fails.
                        try { fault("before-native"); key = job.run(secret, proof, confirmed, sink) }
                        catch (error: Exception) { throw sink.failure?.let { Failure(it) } ?: error }
                        finally { if (output.isOpen) output.close() }
                        fault("after-native")
                        operation.check(); syncDirectory(directory); barrier(review.epoch); operation.check()
                        synchronized(lock) {
                            operation.check()
                            Prepared(file!!, checkNotNull(key), review.epoch, sink.count,
                                review.includesOriginalInk, review.sourceAssets, review.omissions, sink.digest.digest()).also {
                                prepared = it; file = null; key = null
                            }
                        }
                    }
                } finally {
                    secret.fill('\u0000'); key?.fill('\u0000')
                    file?.let { removePrivate(it) }
                    synchronized(lock) { operation.nativeJob = null }
                    review.close(); try { review.native.close() } catch (_: Exception) { }
                }
            }
        } catch (error: Throwable) { secret.fill('\u0000'); throw error }
    }
    private fun <T> withProviderSignal(operation: Operation<*>, action: (CancellationSignal) -> T): T {
        val signal = operation.providerSignal()
        val timer = deadlines.schedule({ operation.cancel() }, operation.remainingNanos(), TimeUnit.NANOSECONDS)
        return try { action(signal) } finally { timer.cancel(false) }
    }
    private fun reviewProviderDestination(artifact: Prepared, tree: Uri, flags: Int, timeout: Long): Pending<ProviderDestination> {
        val required = Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
        require(tree.scheme == "content" && tree.toString().length <= 4096 && DocumentsContract.isTreeUri(tree) && flags and required == required)
        synchronized(lock) { check(!closed && artifact === prepared && artifact.live) }
        return submit(timeout, { it.close() }) { operation ->
            withProviderSignal(operation) { signal ->
                val observation = documents.inspect(tree, signal)
                require(observation.identity == DocumentsContract.getTreeDocumentId(tree) && observation.label.length <= 4096)
                operation.check()
                synchronized(lock) {
                    operation.check(); check(artifact.live && prepared === artifact)
                    destination?.close()
                    ProviderDestination(artifact, tree, observation).also { destination = it }
                }
            }
        }
    }
    private fun verifyCiphertext(artifact: Prepared, operation: Operation<*>) {
        FileChannel.open(artifact.file.toPath(), StandardOpenOption.READ, LinkOption.NOFOLLOW_LINKS).use { source ->
            val buffer = ByteBuffer.allocate(64 * 1024)
            val digest = java.security.MessageDigest.getInstance("SHA-256")
            var total = 0L
            while (true) {
                operation.check(); buffer.clear()
                val count = source.read(buffer)
                if (count < 0) break
                if (count.toLong() > artifact.bytes - total) throw Failure(FailureReason.IntegrityFailed)
                digest.update(buffer.array(), 0, count); total += count
            }
            if (total != artifact.bytes || !java.security.MessageDigest.isEqual(digest.digest(), artifact.digest))
                throw Failure(FailureReason.IntegrityFailed)
        }
    }
    private fun publishProvider(selected: ProviderDestination, confirmed: Boolean, timeout: Long): Pending<ProviderPublication> {
        require(confirmed)
        synchronized(lock) { check(!closed && destination === selected && selected.live && selected.artifact.live) }
        return submit(timeout, {}) { operation ->
            withProviderSignal(operation) { signal ->
                val artifact = selected.artifact
                if (documents.inspect(selected.tree, signal) != selected.observed) throw Failure(FailureReason.SourceChanged)
                verifyCiphertext(artifact, operation)
                var created: Uri? = null
                var syncAcknowledged = false
                try {
                    synchronized(lock) {
                        operation.check(); check(destination === selected && selected.live && artifact.live)
                        selected.close()
                        // A provider can mutate externally before create returns. From
                        // here cancellation/errors must report an uncertain outcome.
                        operation.partialOutcome = true
                    }
                    val document = documents.create(selected.tree, "STEM-backup-${java.util.UUID.randomUUID()}.stemenc")
                    require(document.scheme == "content" && document.toString().length <= 4096 && document.authority == selected.tree.authority)
                    require(DocumentsContract.getTreeDocumentId(document) == selected.observed.identity)
                    require(DocumentsContract.getDocumentId(document) != selected.observed.identity)
                    created = document
                    operation.check()
                    check(documents.isChild(selected.tree, document))
                    val outputDescriptor = documents.open(document, true, signal)
                    try {
                        ParcelFileDescriptor.AutoCloseOutputStream(outputDescriptor).use { output ->
                            FileChannel.open(artifact.file.toPath(), StandardOpenOption.READ, LinkOption.NOFOLLOW_LINKS).use { source ->
                                val buffer = ByteBuffer.allocate(64 * 1024)
                                val digest = java.security.MessageDigest.getInstance("SHA-256")
                                var total = 0L
                                while (true) {
                                    operation.check(); buffer.clear()
                                    val count = source.read(buffer)
                                    if (count < 0) break
                                    if (count.toLong() > artifact.bytes - total) throw Failure(FailureReason.IntegrityFailed)
                                    output.write(buffer.array(), 0, count); digest.update(buffer.array(), 0, count); total += count
                                }
                                if (total != artifact.bytes || !java.security.MessageDigest.isEqual(digest.digest(), artifact.digest))
                                    throw Failure(FailureReason.IntegrityFailed)
                            }
                            operation.check(); output.flush(); outputDescriptor.checkError()
                            try { fault("provider-sync"); outputDescriptor.fileDescriptor.sync(); syncAcknowledged = true } catch (_: java.io.SyncFailedException) { }
                        }
                    } finally { outputDescriptor.close() }
                    operation.check(); fault("provider-written")
                    val inputDescriptor = documents.open(document, false, signal)
                    try {
                        ParcelFileDescriptor.AutoCloseInputStream(inputDescriptor).use { input ->
                            val bytes = ByteArray(64 * 1024)
                            val digest = java.security.MessageDigest.getInstance("SHA-256")
                            var total = 0L
                            while (true) {
                                operation.check()
                                val count = input.read(bytes)
                                if (count < 0) break
                                if (count.toLong() > artifact.bytes - total) throw Failure(FailureReason.IntegrityFailed)
                                digest.update(bytes, 0, count); total += count
                            }
                            inputDescriptor.checkError()
                            if (total != artifact.bytes || !java.security.MessageDigest.isEqual(digest.digest(), artifact.digest))
                                throw Failure(FailureReason.IntegrityFailed)
                        }
                    } finally { inputDescriptor.close() }
                    operation.check()
                    ProviderPublication(document, artifact.bytes, Outcome.ProviderVerified, syncAcknowledged,
                        try { current(artifact.epoch) } catch (_: Exception) { false })
                } catch (_: Exception) {
                    if (!operation.partialOutcome) throw Failure(FailureReason.Unavailable)
                    needsAttention = true
                    ProviderPublication(created, artifact.bytes, Outcome.Uncertain, false, false)
                }
            }
        }
    }
    private fun publishPrepared(artifact: Prepared, destination: File, confirmed: Boolean, timeout: Long): Pending<Publication> {
        require(confirmed && destination.isAbsolute)
        val target = destination.toPath()
        val parent = checkNotNull(destination.absoluteFile.parentFile)
        // Destination is a deliberate trusted local-file capability, never JSON.
        synchronized(lock) { check(!closed && prepared === artifact && artifact.live) }
        return submit(timeout, {}) { operation ->
            operation.check()
            require(parent.isDirectory && !Files.isSymbolicLink(parent.toPath()) && !Files.isSymbolicLink(target))
            require(!parent.canonicalFile.toPath().startsWith(privateRoot.canonicalFile.toPath()))
            synchronized(lock) { check(prepared === artifact && artifact.live) }
            var pending: File? = null
            var linked = false
            try {
                if (Files.exists(target, LinkOption.NOFOLLOW_LINKS)) throw Failure(FailureReason.DestinationExists)
                val copy = Files.createTempFile(parent.toPath(), ".stem-backup-", ".pending")
                pending = copy.toFile()
                val digest = java.security.MessageDigest.getInstance("SHA-256")
                var copied = 0L
                FileChannel.open(artifact.file.toPath(), StandardOpenOption.READ, LinkOption.NOFOLLOW_LINKS).use { source ->
                    FileChannel.open(copy, StandardOpenOption.WRITE, LinkOption.NOFOLLOW_LINKS).use { output ->
                        val buffer = ByteBuffer.allocate(64 * 1024)
                        while (true) {
                            operation.check(); buffer.clear()
                            val read = source.read(buffer)
                            if (read < 0) break
                            if (read.toLong() > artifact.bytes - copied) throw Failure(FailureReason.IntegrityFailed)
                            digest.update(buffer.array(), 0, read); copied += read
                            buffer.flip()
                            while (buffer.hasRemaining()) { operation.check(); output.write(buffer) }
                        }
                        if (copied != artifact.bytes || !java.security.MessageDigest.isEqual(digest.digest(), artifact.digest)) throw Failure(FailureReason.IntegrityFailed)
                        fault("publish-retire"); output.force(true)
                    }
                }
                operation.check()
                synchronized(lock) {
                    operation.check(); check(artifact.live)
                    // Hard-link publication is atomic and CREATE_NEW. Atomic move
                    // alone may overwrite an existing destination on Unix.
                    operation.committing = true
                }
                Files.createLink(target, copy); linked = true
                fault("publish-link"); syncDirectory(parent)
                check(Files.deleteIfExists(copy)); pending = null; syncDirectory(parent)
                Publication(artifact.bytes, Outcome.Published, try { current(artifact.epoch) } catch (_: Exception) { false })
            } catch (error: Exception) {
                if (linked) { needsAttention = true; Publication(artifact.bytes, Outcome.Uncertain, false) } else throw error
            } finally {
                pending?.let { try { if (!it.delete()) needsAttention = true; syncDirectory(parent) } catch (_: Exception) { needsAttention = true } }
            }
        }
    }
    // lock held; same worker serialises deletion after all admitted file I/O.
    private fun queueCleanup(artifact: Prepared) {
        if (artifact.cleanupQueued) return
        artifact.cleanupQueued = true
        worker.execute {
            try { removePrivate(artifact.file) }
            finally { synchronized(lock) { if (prepared === artifact) prepared = null } }
        }
    }
    private fun removePrivate(file: File) {
        try { fault("stage-delete"); Files.deleteIfExists(file.toPath()); syncDirectory(directory) }
        catch (_: Exception) { needsAttention = true }
    }
    private fun syncDirectory(directory: File) = FileChannel.open(directory.toPath(), StandardOpenOption.READ).use { it.force(true) }
    /** Retire uncommitted export capabilities on access/containment/deletion policy
     * changes. Already committing publication drains to an explicit outcome. */
    internal fun invalidate() {
        val old = synchronized(lock) {
            if (closed) return
            prepared?.close()
            running to review
        }
        old.first?.cancel(); old.second?.close()
    }
    override fun close() {
        val old = synchronized(lock) {
            if (closed) return
            closed = true
            prepared?.let { it.live = false; it.close() }
            val pair = running to review
            worker.shutdown()
            pair
        }
        old.first?.cancel(); old.second?.close()
        deadlines.shutdownNow(); cancellations.shutdown()
    }
    internal fun awaitClosed() = awaitStopped(worker, cancellations, deadlines)
}
