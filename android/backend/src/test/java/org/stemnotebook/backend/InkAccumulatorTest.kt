package org.stemnotebook.backend

import org.junit.Assert.*
import org.junit.Test

class InkAccumulatorTest {
    private fun point(time: Long, x: Double = 0.125) = ObservedPoint(x, -0.0, time, null, null, null, null)
    @Test fun chosenStyleBelongsToBeginAndLegacyEncodingDoesNotInventIt() {
        val a = InkAccumulator(); val first = StrokeStyle(0, 128, 255, 64, 0.12345678901234567); val second = first.copy(width = 9.5)
        a.setStyle(first); a.begin(1, false, point(0)); a.setStyle(second); a.append(1, listOf(point(1))); a.begin(2, true, point(0))
        val completed = a.complete(1); assertEquals(first, completed.style); assertEquals(second, a.complete(2).style)
        assertFalse(CaptureRequest.encode(java.util.UUID.randomUUID(), java.util.UUID.randomUUID(), "space", listOf(completed.copy(style = null))).contains("\"style\""))
        assertThrows(IllegalArgumentException::class.java) { first.copy(width = Double.NaN) }; assertThrows(IllegalArgumentException::class.java) { first.copy(red = 256) }
    }
    @Test fun preservesOrderZeroAndUnavailableChannels() {
        val a = InkAccumulator(); a.begin(2, false, point(10)); a.append(2, listOf(point(11), point(12)))
        val stroke = a.complete(2)
        assertEquals(listOf(10L, 11L, 12L), stroke.points.map { it.timeUs })
        assertEquals((-0.0).toRawBits(), stroke.points[0].y.toRawBits())
        assertNull(stroke.points[0].pressure)
        assertTrue(a.pendingPointers().isEmpty())
    }
    @Test fun cancellationDiscardsOnlyTentativePointer() {
        val a=InkAccumulator();a.begin(1,false,point(0));a.begin(2,true,point(0));a.cancel(1)
        assertEquals(setOf(2),a.pendingPointers());assertTrue(a.complete(2).eraser)
        a.begin(3,false,point(1));a.cancelAll();assertTrue(a.pendingPointers().isEmpty())
    }
    @Test fun malformedAndOversizedBatchesDoNotPartiallyAppend() {
        val a=InkAccumulator(3);a.begin(1,false,point(10))
        try {a.append(1,listOf(point(12),point(9)));fail()}catch (_:IllegalArgumentException){}
        try {a.append(1,listOf(point(12),point(13),point(14)));fail()}catch (_:IllegalArgumentException){}
        try {a.append(1,listOf(point(12,Double.NaN)));fail()}catch (_:IllegalArgumentException){}
        assertEquals(listOf(point(10)),a.complete(1).points)
    }
    @Test fun completedStrokesHaveStableSessionAndDistinctMonotonicIdentities() {
        val a=InkAccumulator();a.begin(1,false,point(0));val first=a.complete(1)
        a.begin(1,false,point(1));val second=a.complete(1)
        assertEquals(first.session,second.session);assertNotEquals(first.id,second.id);assertEquals(1,second.sequence)
    }
    @Test fun activeCheckpointsHaveStableBeginIdentityAndNoFabricatedCompletion() {
        val a = InkAccumulator(3)
        a.begin(9, false, point(10)); a.begin(1, true, point(10))
        val original = a.checkpoint()
        assertEquals(listOf(9, 1), original.map { it.pointer })
        assertTrue(original.all { it.end == StrokeEnd.InProgress })
        a.append(9, listOf(point(11)))
        assertThrows(IllegalArgumentException::class.java) { a.append(1, listOf(point(12))) }
        val latest = a.checkpoint(); val interrupted = a.interrupt(StrokeInterruption.Lifecycle)
        assertTrue(a.pendingPointers().isEmpty()); assertEquals(original[0].id, latest[0].id)
        assertEquals(1, original[0].points.size)
        assertEquals(latest.map { it.points }, interrupted.map { it.points })
        assertTrue(interrupted.all { it.end == StrokeEnd.Interrupted(StrokeInterruption.Lifecycle) })
        assertEquals((-0.0).toRawBits(), interrupted[0].points[0].y.toRawBits())
        a.begin(2, false, point(20)); val prefix = a.checkpoint().single(); val complete = a.complete(2)
        assertEquals(prefix.id, complete.id); assertEquals(2L, complete.sequence); assertEquals(StrokeEnd.Completed, complete.end)
    }
    @Test fun documentCaptureEncoderRejectsActivePrefixesAndPreservesExplicitInterruption() {
        val a = InkAccumulator(); a.begin(1, false, point(10))
        val ticket = java.util.UUID.randomUUID(); val entity = java.util.UUID.randomUUID()
        assertThrows(IllegalArgumentException::class.java) { CaptureRequest.encode(ticket, entity, "fixed-space", a.checkpoint()) }
        val interrupted = a.interrupt(StrokeInterruption.Recovery)
        val request = CaptureRequest.encode(ticket, entity, "fixed-space", interrupted)
        assertTrue(request.contains("\"end\":{\"Interrupted\":\"Recovery\"}"))
        a.begin(1, false, point(20))
        assertFalse(CaptureRequest.encode(ticket, entity, "fixed-space", listOf(a.complete(1))).contains("\"end\""))
    }

}
