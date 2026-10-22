package org.stemnotebook.backend

import org.junit.Assert.*
import org.junit.Test

class InkAccumulatorTest {
    private fun point(time: Long, x: Double = 0.125) = ObservedPoint(x, -0.0, time, null, null, null, null)
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
}
