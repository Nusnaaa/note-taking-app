package org.stemnotebook.backend

import android.view.MotionEvent
import java.util.UUID
import java.util.concurrent.CompletableFuture

/** Owns input identity and stale-frame admission. Rejection returns the caller's
 * immutable observations; checkpointed is distinct from a SQLite commit. */
class JournaledCaptureSession internal constructor(internal val backend: NotebookBackend, internal val entity: UUID, internal val space: String, style: StrokeStyle? = null, val layer: CaptureLayer? = null, fingerInk: Boolean = false, val cell: CaptureCell? = null) {
    val id: UUID = UUID.randomUUID()
    private val motion = MotionCapture(InkAccumulator(sessionId = id), fingerInk).apply { setStyle(style) }
    private var revision = 0L
    private var closed = false
    internal var latestSequence = -1L // writer-executor access only
    internal var nextCell: CaptureCell? = cell // writer executor only
    internal class CellBinding(val session: UUID, val ticket: UUID) { var target: CaptureCell? = null }
    private val bindingLock = Any()
    private val cellBindings = mutableMapOf<UUID, java.lang.ref.WeakReference<CellBinding>>()
    internal fun cellBinding(ticket: UUID): CellBinding = synchronized(bindingLock) {
        cellBindings.entries.removeAll { it.value.get() == null }
        cellBindings[ticket]?.get() ?: run {
            check(cellBindings.size < 256)
            CellBinding(id, ticket).also { cellBindings[ticket] = java.lang.ref.WeakReference(it) }
        }
    }
    internal fun retireCellBinding(ticket: UUID) { synchronized(bindingLock) { cellBindings.remove(ticket) } }
    internal val seen = mutableMapOf<UUID, Long>() // bounded by journal admission
    private var interrupted = emptyList<CapturedStroke>()
    data class Receipt(val ticket: UUID, val revision: Long, val checkpointed: Boolean, val committed: Boolean, val discarded: Boolean = false)
    data class Submission(val stroke: CapturedStroke, val revision: Long, private val completion: CompletableFuture<Receipt>, val cancelled: Boolean = false) {
        internal var cellBinding: CellBinding? = null
        // Cancelling observation never retires an admitted write or its priority.
        val receipt: CompletableFuture<Receipt> = completion.thenApply { it }
        internal val retirement: CompletableFuture<Receipt> get() = completion
    }
    /** New contacts inherit this style; observed active prefixes never change. */
    @Synchronized fun setStyle(style: StrokeStyle?) { check(!closed); motion.setStyle(style) }
    /** Applies to future contacts; active contact provenance remains immutable. */
    @Synchronized fun setFingerInk(enabled: Boolean) { check(!closed); motion.setFingerInk(enabled) }
    @Synchronized fun accept(event: MotionEvent): List<Submission> {
        check(!closed); check(revision < Long.MAX_VALUE); revision++
        val before = motion.checkpoint(); val completed = motion.accept(event); val active = motion.checkpoint()
        val retained = (completed + active).map { it.id }.toSet()
        val cancelled = before.filter { it.id !in retained }
        if (before.isNotEmpty() || completed.isNotEmpty() || active.isNotEmpty()) backend.beginCapturePriority(this, revision)
        val result = ((completed + active).map { it to false } + cancelled.map { it to true }).sortedBy { it.first.sequence }.map { (stroke, cancel) -> submit(stroke, revision, cancel) }
        if (active.isEmpty()) backend.releaseCapturePriorityAfter(this, revision, result.map { it.retirement })
        return result
    }
    /** Retry the same immutable frame after queue/storage failure. */
    @Synchronized fun retry(value: Submission): Submission { require(!closed || (!value.cancelled && value.stroke.end != StrokeEnd.InProgress)); return submit(value.stroke, value.revision, value.cancelled, value.cellBinding) }
    private fun submit(stroke: CapturedStroke, version: Long, cancelled: Boolean, existing: CellBinding? = null): Submission {
        var binding = existing
        val future = try {
            if (cell != null && binding == null) binding = cellBinding(UUID.fromString(stroke.id))
            backend.checkpointCapture(this, stroke, version, cancelled, binding)
        } catch (error: Exception) { CompletableFuture<Receipt>().apply { completeExceptionally(error) } }
        return Submission(stroke, version, future, cancelled).also { it.cellBinding = binding }
    }
    /** Lifecycle interruption preserves observed prefixes. Completion may still be
     * pending in recovery storage; callers inspect each returned receipt. */
    @Synchronized fun interrupt(): List<Submission> {
        check(!closed); check(revision < Long.MAX_VALUE); revision++
        interrupted = motion.interrupt()
        if (interrupted.isNotEmpty()) backend.beginCapturePriority(this, revision)
        val result = interrupted.map { submit(it, revision, false) }
        backend.releaseCapturePriorityAfter(this, revision, result.map { it.retirement })
        closed = true; backend.retireCaptureSession(this)
        return result
    }
    /** Caller-owned latest prefixes retained even if close cannot write storage. */
    @Synchronized fun interruptedObservations(): List<CapturedStroke> = interrupted.toList()
    internal data class Closing(val stroke: CapturedStroke, val revision: Long)
    @Synchronized internal fun freezeForClose(): List<Closing> {
        if (closed) return emptyList()
        check(revision < Long.MAX_VALUE); revision++; closed = true
        interrupted = motion.interrupt()
        return interrupted.map { Closing(it, revision) }
    }
}
