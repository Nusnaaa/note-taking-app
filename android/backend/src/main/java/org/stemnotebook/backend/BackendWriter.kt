package org.stemnotebook.backend

import java.util.ArrayDeque
import java.util.concurrent.AbstractExecutorService
import java.util.concurrent.CompletableFuture
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.TimeUnit

/** Content-free admission counters, not a recovery or document-save receipt. */
data class BackendAdmissionStatus(
    val sequence: Long,
    val pendingAuthoring: Int,
    val authoringBytes: Long,
    val pendingQueries: Int,
    val queryBytes: Long,
    val needsAttention: Boolean,
    val closed: Boolean,
    val terminated: Boolean,
)

/** Pull-based coalescing; no callbacks or per-observer event queue on the writer.
 * Call next from an observer thread; slow observers receive the latest snapshot. */
class BackendStatusSubscription internal constructor(private val writer: BackendWriter) {
    private var seen: Long? = null
    @Synchronized fun next(timeoutMillis: Long = 0): BackendAdmissionStatus? {
        require(timeoutMillis in 0..120_000)
        return writer.observe(seen, TimeUnit.MILLISECONDS.toNanos(timeoutMillis))?.also { seen = it.sequence }
    }
}

/** One draining writer with reserved authoring/query count and input-byte budgets.
 * Counts include the running operation. FIFO edits/access changes precede queued
 * reads; a running operation is never preempted. No admitted work is discarded. */
internal class BackendWriter(
    private val limits: Limits = Limits(),
) : AbstractExecutorService() {
    internal data class Limits(val authoringCount: Int = 64, val authoringBytes: Long = 32L * 1024 * 1024, val queryCount: Int = 16, val queryBytes: Long = 8L * 1024 * 1024) {
        init {
            require(authoringCount in 1..4096 && queryCount in 1..4096)
            require(authoringBytes in 1..256L * 1024 * 1024 && queryBytes in 1..256L * 1024 * 1024)
        }
    }
    private data class Completion(val failed: Boolean, val publish: () -> Unit)
    private data class Work(val authoring: Boolean, val bytes: Long, val run: () -> Completion)
    private val lock = Object()
    private val authoring = ArrayDeque<Work>()
    private val queries = ArrayDeque<Work>()
    private var state = BackendAdmissionStatus(0, 0, 0, 0, 0, false, false, false)
    private var started = false
    private val thread = Thread({ drain() }, "stem-backend-writer").apply { isDaemon = true }

    fun snapshot(): BackendAdmissionStatus = synchronized(lock) { state }
    fun subscribe() = BackendStatusSubscription(this)
    fun markAttention() { synchronized(lock) { if (!state.needsAttention) publish(state.copy(needsAttention = true)) } }
    private fun publish(next: BackendAdmissionStatus) { state = next.copy(sequence = state.sequence + 1); lock.notifyAll() }
    internal fun observe(seen: Long?, timeoutNanos: Long): BackendAdmissionStatus? = synchronized(lock) {
        val start = System.nanoTime()
        var remaining = timeoutNanos
        while (state.sequence == seen && !state.closed && remaining > 0) {
            TimeUnit.NANOSECONDS.timedWait(lock, remaining)
            remaining = timeoutNanos - (System.nanoTime() - start)
        }
        if (state.sequence == seen) null else state
    }
    fun <T> admit(authoring: Boolean, bytes: Long, action: () -> T): CompletableFuture<T> {
        val result = CompletableFuture<T>()
        enqueue(Work(authoring, bytes) {
            try { val value = action(); Completion(false) { result.complete(value); Unit } }
            catch (error: Throwable) { Completion(true) { result.completeExceptionally(error); Unit } }
        })
        return result
    }
    override fun execute(command: Runnable) = enqueue(Work(true, 256) {
        try { command.run(); Completion(false) {} } catch (_: Throwable) { Completion(true) {} }
    })
    private fun enqueue(work: Work) = synchronized(lock) {
        require(work.bytes > 0)
        val count = if (work.authoring) state.pendingAuthoring else state.pendingQueries
        val bytes = if (work.authoring) state.authoringBytes else state.queryBytes
        val countLimit = if (work.authoring) limits.authoringCount else limits.queryCount
        val byteLimit = if (work.authoring) limits.authoringBytes else limits.queryBytes
        if (state.closed || count >= countLimit || work.bytes > byteLimit - bytes) throw RejectedExecutionException("Backend writer capacity unavailable")
        val queue = if (work.authoring) authoring else queries
        queue.addLast(work)
        if (!started) {
            try { thread.start(); started = true } catch (error: Throwable) { queue.removeLast(); throw error }
        }
        if (work.authoring) publish(state.copy(pendingAuthoring = count + 1, authoringBytes = bytes + work.bytes))
        else publish(state.copy(pendingQueries = count + 1, queryBytes = bytes + work.bytes))
    }
    private fun drain() {
        try {
            while (true) {
                val work = synchronized(lock) {
                    while (authoring.isEmpty() && queries.isEmpty() && !state.closed) lock.wait()
                    if (authoring.isEmpty() && queries.isEmpty()) return
                    if (authoring.isNotEmpty()) authoring.removeFirst() else queries.removeFirst()
                }
                val result = work.run()
                synchronized(lock) {
                    if (work.authoring) publish(state.copy(pendingAuthoring = state.pendingAuthoring - 1, authoringBytes = state.authoringBytes - work.bytes, needsAttention = state.needsAttention || result.failed))
                    else publish(state.copy(pendingQueries = state.pendingQueries - 1, queryBytes = state.queryBytes - work.bytes))
                }
                // Cancellation only cancels observation. Complete outside the monitor
                // after accounting retires; dependent callbacks cannot hold its lock.
                result.publish()
            }
        } finally {
            synchronized(lock) { publish(state.copy(closed = true, terminated = true)) }
        }
    }
    override fun shutdown() { synchronized(lock) { if (!state.closed) publish(state.copy(closed = true, terminated = !started)) } }
    /** This writer deliberately drains even on shutdownNow; interruption cannot
     * safely retract admitted ink or an in-flight native transaction. */
    override fun shutdownNow(): MutableList<Runnable> { shutdown(); return mutableListOf() }
    override fun isShutdown(): Boolean = synchronized(lock) { state.closed }
    override fun isTerminated(): Boolean = synchronized(lock) { state.terminated }
    override fun awaitTermination(timeout: Long, unit: TimeUnit): Boolean = synchronized(lock) {
        val duration = unit.toNanos(timeout).coerceAtLeast(0); val start = System.nanoTime(); var remaining = duration
        while (!state.terminated && remaining > 0) {
            TimeUnit.NANOSECONDS.timedWait(lock, remaining)
            remaining = duration - (System.nanoTime() - start)
        }
        state.terminated
    }
}
