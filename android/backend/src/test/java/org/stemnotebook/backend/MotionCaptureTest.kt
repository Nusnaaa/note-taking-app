package org.stemnotebook.backend

import android.view.InputDevice
import android.view.MotionEvent
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], manifest = Config.NONE)
class MotionCaptureTest {
    private fun event(action: Int, time: Long, tool: Int = MotionEvent.TOOL_TYPE_STYLUS): MotionEvent {
        val property = MotionEvent.PointerProperties().apply { id = 7; toolType = tool }
        val coordinate = MotionEvent.PointerCoords().apply { x = 0.125f; y = -0.0f; pressure = 0.5f }
        return MotionEvent.obtain(10, time, action, 1, arrayOf(property), arrayOf(coordinate), 0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_STYLUS, 0)
    }
    @Test fun observedAndroidEventsProduceOneOrderedSourceStroke() {
        val capture=MotionCapture()
        var observedYBits = 0L
        event(MotionEvent.ACTION_DOWN,10).useEvent {
            // Android may normalise signed zero before delivering MotionEvent.
            // The source contract preserves the value actually exposed by its API.
            observedYBits = it.getY(0).toDouble().toRawBits()
            assertTrue(capture.accept(it).isEmpty())
        }
        event(MotionEvent.ACTION_MOVE,20).useEvent { move ->
            val point=MotionEvent.PointerCoords().apply {x=2.5f;y=3.0f;pressure=0.6f}
            move.addBatch(30,arrayOf(point),0);assertTrue(capture.accept(move).isEmpty())
        }
        event(MotionEvent.ACTION_UP,40).useEvent { up ->
            val stroke=capture.accept(up).single();assertEquals(listOf(0L,10_000L,20_000L,30_000L),stroke.points.map {it.timeUs})
            assertEquals(0.125,stroke.points[0].x,0.0);assertEquals(2.5,stroke.points[2].x,0.0)
            assertEquals(observedYBits,stroke.points[0].y.toRawBits())
        }
    }
    @Test fun cancellationAndFingerContactNeverCommitPenInk() {
        val capture=MotionCapture()
        event(MotionEvent.ACTION_DOWN,10).useEvent {capture.accept(it)}
        event(MotionEvent.ACTION_CANCEL,11).useEvent {assertTrue(capture.accept(it).isEmpty())}
        event(MotionEvent.ACTION_UP,12).useEvent {assertTrue(capture.accept(it).isEmpty())}
        event(MotionEvent.ACTION_DOWN,13,MotionEvent.TOOL_TYPE_FINGER).useEvent {assertTrue(capture.accept(it).isEmpty())}
        event(MotionEvent.ACTION_UP,14,MotionEvent.TOOL_TYPE_FINGER).useEvent {assertTrue(capture.accept(it).isEmpty())}
    }
    @Test fun lifecycleInterruptionSealsOnlyObservedPrefixAndCannotBeConfusedWithPlatformCancel() {
        val capture = MotionCapture()
        event(MotionEvent.ACTION_DOWN, 10).useEvent { capture.accept(it) }
        event(MotionEvent.ACTION_MOVE, 20).useEvent { capture.accept(it) }
        val prefix = capture.checkpoint().single()
        val interrupted = capture.interrupt().single()
        assertEquals(prefix.id, interrupted.id); assertEquals(prefix.points, interrupted.points)
        assertEquals(listOf(0L, 10_000L), interrupted.points.map { it.timeUs })
        assertEquals(StrokeEnd.Interrupted(StrokeInterruption.Lifecycle), interrupted.end)
        event(MotionEvent.ACTION_UP, 30).useEvent { assertTrue(capture.accept(it).isEmpty()) }
        assertTrue(capture.checkpoint().isEmpty())
        event(MotionEvent.ACTION_DOWN, 40).useEvent { capture.accept(it) }
        event(MotionEvent.ACTION_CANCEL, 50).useEvent { capture.accept(it) }
        assertTrue(capture.interrupt().isEmpty())
    }
    private inline fun MotionEvent.useEvent(action: (MotionEvent)->Unit) {try {action(this)}finally{recycle()}}
}
