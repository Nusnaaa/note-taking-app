package org.stemnotebook.backend

import java.util.UUID

/** Chosen appearance in the named source-coordinate units; never inferred from
 * pressure or current view settings. Immutable channels prevent caller rebinding. */
data class StrokeStyle(val red: Int, val green: Int, val blue: Int, val alpha: Int, val width: Double) {
    init { require(listOf(red, green, blue, alpha).all { it in 0..255 } && width.isFinite() && width > 0 && width <= 1_000_000) }
}
enum class StrokeInterruption { Lifecycle, Recovery }
sealed class StrokeEnd {
    data object Completed : StrokeEnd()
    data object InProgress : StrokeEnd()
    data class Interrupted(val reason: StrokeInterruption) : StrokeEnd()
}
/** Exact observations and provenance. Completion is not a save acknowledgement. */
data class CapturedStroke(val id: String, val session: String, val sequence: Long, val pointer: Int, val eraser: Boolean, val points: List<ObservedPoint>, val end: StrokeEnd = StrokeEnd.Completed, val style: StrokeStyle? = null)
/** Source compatibility for the previous completed-only capture adapter. */
typealias CompletedStroke = CapturedStroke
class InkAccumulator(private val maximumPoints: Int = 100_000, sessionId: UUID = UUID.randomUUID()) {
    private val session = sessionId.toString()
    private var sequence = 0L
    private var style: StrokeStyle? = null
    fun setStyle(value: StrokeStyle?) { style = value }
    private data class Active(val id: String, val sequence: Long, val pointer: Int, val eraser: Boolean, val points: MutableList<ObservedPoint>, val style: StrokeStyle?)
    private val active = mutableMapOf<Int, Active>()
    init { require(maximumPoints in 1..100_000) }
    fun begin(pointer: Int, eraser: Boolean, sample: ObservedPoint) {
        require(pointer >= 0); check(pointer !in active)
        validate(sample)
        require(active.size < 32 && active.values.sumOf { it.points.size } < maximumPoints)
        check(sequence < Long.MAX_VALUE)
        active[pointer] = Active(UUID.randomUUID().toString(), sequence++, pointer, eraser, mutableListOf(sample), style)
    }
    fun append(pointer: Int, samples: List<ObservedPoint>) {
        val target = active[pointer]?.points ?: error("Pointer unavailable")
        require(active.values.sumOf { it.points.size.toLong() } + samples.size <= maximumPoints)
        var last = target.last().timeUs
        for (point in samples) { validate(point); require(point.timeUs >= last); last = point.timeUs }
        target.addAll(samples)
    }
    private fun snapshot(value: Active, end: StrokeEnd) = CapturedStroke(value.id, session, value.sequence, value.pointer, value.eraser, value.points.toList(), end, value.style)
    fun complete(pointer: Int): CapturedStroke = snapshot(active.remove(pointer) ?: error("Pointer unavailable"), StrokeEnd.Completed)
    /** Stable IDs/begin-order, immutable observed prefixes; not a save receipt. */
    fun checkpoint(): List<CapturedStroke> = active.values.sortedBy { it.sequence }.map { snapshot(it, StrokeEnd.InProgress) }
    /** Seals the observed prefix without synthesising pen-up samples or timestamps. */
    fun interrupt(reason: StrokeInterruption): List<CapturedStroke> {
        val result = active.values.sortedBy { it.sequence }.map { snapshot(it, StrokeEnd.Interrupted(reason)) }
        active.clear()
        return result
    }
    fun cancel(pointer: Int) { active.remove(pointer) }
    fun cancelAll() { active.clear() }
    fun pendingPointers(): Set<Int> = active.keys.toSet()
    private fun validate(point: ObservedPoint) {
        require(point.x.isFinite() && point.y.isFinite() && point.timeUs >= 0)
        require(listOf(point.pressure, point.tiltRad, point.orientationRad).filterNotNull().all { it.isFinite() })
    }
}
