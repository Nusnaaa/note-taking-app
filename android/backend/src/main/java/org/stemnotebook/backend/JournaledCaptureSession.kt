package org.stemnotebook.backend

import android.view.MotionEvent
import java.util.UUID
import java.util.concurrent.CompletableFuture

/** Owns input identity and stale-frame admission. Rejection returns the caller's
 * immutable observations; checkpointed is distinct from a SQLite commit. */
class JournaledCaptureSession internal constructor(internal val backend: NotebookBackend, internal val entity: UUID, internal val space: String, style: StrokeStyle? = null, val layer: CaptureLayer? = null, fingerInk: Boolean = false) {
    val id: UUID = UUID.randomUUID()
    private val motion = MotionCapture(InkAccumulator(sessionId = id), fingerInk).apply { setStyle(style) }
    private var revision = 0L
    private var closed = false
    internal var latestSequence = -1L // writer-executor access only
    internal val seen = mutableMapOf<UUID, Long>() // bounded by journal admission
    private var interrupted = emptyList<CapturedStroke>()
    data class Receipt(val ticket: UUID, val revision: Long, val checkpointed: Boolean, val committed: Boolean, val discarded: Boolean = false)
    data class Submission(val stroke: CapturedStroke, val revision: Long, val receipt: CompletableFuture<Receipt>, val cancelled: Boolean = false)
    /** New contacts inherit this style; observed active prefixes never change. */
    @Synchronized fun setStyle(style: StrokeStyle?) { check(!closed); motion.setStyle(style) }
    /** Applies to future contacts; active contact provenance remains immutable. */
    @Synchronized fun setFingerInk(enabled: Boolean) { check(!closed); motion.setFingerInk(enabled) }
    @Synchronized fun accept(event: MotionEvent): List<Submission> {
        check(!closed); check(revision < Long.MAX_VALUE); revision++
        val before = motion.checkpoint(); val completed = motion.accept(event); val active = motion.checkpoint()
        val retained = (completed + active).map { it.id }.toSet()
        val cancelled = before.filter { it.id !in retained }
        return ((completed + active).map { it to false } + cancelled.map { it to true }).sortedBy { it.first.sequence }.map { (stroke, cancel) -> submit(stroke, revision, cancel) }
    }
    /** Retry the same immutable frame after queue/storage failure. */
    @Synchronized fun retry(value: Submission): Submission { require(!closed || (!value.cancelled && value.stroke.end != StrokeEnd.InProgress)); return submit(value.stroke, value.revision, value.cancelled) }
    private fun submit(stroke: CapturedStroke, version: Long, cancelled: Boolean): Submission {
        val future = try { backend.checkpointCapture(this, stroke, version, cancelled) } catch (error: Exception) { CompletableFuture<Receipt>().apply { completeExceptionally(error) } }
        return Submission(stroke, version, future, cancelled)
    }
    /** Lifecycle interruption preserves observed prefixes. Completion may still be
     * pending in recovery storage; callers inspect each returned receipt. */
    @Synchronized fun interrupt(): List<Submission> {
        check(!closed); check(revision < Long.MAX_VALUE); revision++
        interrupted = motion.interrupt()
        val result = interrupted.map { submit(it, revision, false) }
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
