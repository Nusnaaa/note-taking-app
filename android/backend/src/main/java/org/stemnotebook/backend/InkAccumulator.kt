package org.stemnotebook.backend

import java.util.UUID

/** Observations survive as vectors. Completion is not a save acknowledgement. */
data class CompletedStroke(val id: String, val session: String, val sequence: Long, val pointer: Int, val eraser: Boolean, val points: List<ObservedPoint>)
class InkAccumulator(private val maximumPoints: Int = 100_000) {
    private val session = UUID.randomUUID().toString()
    private var sequence = 0L
    private val active = mutableMapOf<Int, Pair<Boolean, MutableList<ObservedPoint>>>()
    init { require(maximumPoints in 1..100_000) }
    fun begin(pointer: Int, eraser: Boolean, sample: ObservedPoint) {
        check(pointer !in active)
        validate(sample)
        active[pointer] = eraser to mutableListOf(sample)
    }
    fun append(pointer: Int, samples: List<ObservedPoint>) {
        val target = active[pointer]?.second ?: error("Pointer unavailable")
        // Validate the entire batch before changing the original observations.
        require(target.size + samples.size <= maximumPoints)
        var last = target.last().timeUs
        for (point in samples) { validate(point); require(point.timeUs >= last); last = point.timeUs }
        target.addAll(samples)
    }
    fun complete(pointer: Int): CompletedStroke {
        val (eraser, points) = active.remove(pointer) ?: error("Pointer unavailable")
        check(sequence < Long.MAX_VALUE)
        return CompletedStroke(UUID.randomUUID().toString(), session, sequence++, pointer, eraser, points.toList())
    }
    fun cancel(pointer: Int) { active.remove(pointer) }
    fun cancelAll() { active.clear() }
    fun pendingPointers(): Set<Int> = active.keys.toSet()
    private fun validate(point: ObservedPoint) {
        require(point.x.isFinite() && point.y.isFinite() && point.timeUs >= 0)
        require(listOf(point.pressure, point.tiltRad, point.orientationRad).filterNotNull().all { it.isFinite() })
    }
}
