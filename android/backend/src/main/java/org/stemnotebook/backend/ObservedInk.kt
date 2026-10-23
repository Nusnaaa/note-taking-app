package org.stemnotebook.backend

import android.os.Build
import android.view.MotionEvent

/** Raw observed samples only. Predictions are never accepted or persisted. */
data class ObservedPoint(val x: Double, val y: Double, val timeUs: Long, val pressure: Double?, val tiltRad: Double?, val orientationRad: Double?, val buttons: Int?)
data class ObservedPointer(val id: Int, val tool: Int, val points: List<ObservedPoint>)
object ObservedInk {
    private fun channel(event: MotionEvent, pointer: Int, axis: Int, history: Int?): Double? {
        // A measured zero is distinct from a channel that the device does not expose.
        if (event.device?.getMotionRange(axis, event.source) == null) return null
        return if (history == null) event.getAxisValue(axis, pointer).toDouble()
        else event.getHistoricalAxisValue(axis, pointer, history).toDouble()
    }
    fun read(event: MotionEvent): List<ObservedPointer> {
        if (event.actionMasked == MotionEvent.ACTION_CANCEL) return emptyList()
        return (0 until event.pointerCount).mapNotNull { index ->
            val tool = event.getToolType(index)
            if (tool != MotionEvent.TOOL_TYPE_STYLUS && tool != MotionEvent.TOOL_TYPE_ERASER) return@mapNotNull null
            val points = (0 until event.historySize).map { history ->
                ObservedPoint(event.getHistoricalX(index, history).toDouble(), event.getHistoricalY(index, history).toDouble(), event.getHistoricalEventTime(history) * 1000,
                    channel(event, index, MotionEvent.AXIS_PRESSURE, history), channel(event, index, MotionEvent.AXIS_TILT, history), channel(event, index, MotionEvent.AXIS_ORIENTATION, history), null)
            } + ObservedPoint(event.getX(index).toDouble(), event.getY(index).toDouble(), event.eventTime * 1000,
                channel(event, index, MotionEvent.AXIS_PRESSURE, null), channel(event, index, MotionEvent.AXIS_TILT, null), channel(event, index, MotionEvent.AXIS_ORIENTATION, null), event.buttonState)
            ObservedPointer(event.getPointerId(index), tool, points)
        }
    }
}

/** Bridges Android pointer lifetime, including OS cancellation, to source strokes.
 * Finger contact remains navigation; this adapter never substitutes predicted ink. */
class MotionCapture(private val accumulator: InkAccumulator = InkAccumulator()) {
    private var originUs: Long? = null
    fun checkpoint(): List<CapturedStroke> = accumulator.checkpoint()
    fun interrupt(reason: StrokeInterruption = StrokeInterruption.Lifecycle): List<CapturedStroke> = accumulator.interrupt(reason)
    fun accept(event: MotionEvent): List<CompletedStroke> {
        if (event.actionMasked == MotionEvent.ACTION_CANCEL) { accumulator.cancelAll(); return emptyList() }
        val actionPointer = event.getPointerId(event.actionIndex)
        val cancelled = Build.VERSION.SDK_INT >= 33 && event.flags and MotionEvent.FLAG_CANCELED != 0
        if (cancelled) accumulator.cancel(actionPointer)
        for (pointer in ObservedInk.read(event)) {
            if (cancelled && pointer.id == actionPointer) continue
            if (originUs == null) originUs = pointer.points.first().timeUs
            val points = pointer.points.map { it.copy(timeUs = it.timeUs - originUs!!) }
            val beginning = (event.actionMasked == MotionEvent.ACTION_DOWN || event.actionMasked == MotionEvent.ACTION_POINTER_DOWN) && pointer.id == actionPointer
            if (beginning) {
                accumulator.begin(pointer.id, pointer.tool == MotionEvent.TOOL_TYPE_ERASER, points.first())
                accumulator.append(pointer.id, points.drop(1))
            } else if (pointer.id in accumulator.pendingPointers()) {
                accumulator.append(pointer.id, points)
            }
        }
        val finishing = event.actionMasked == MotionEvent.ACTION_UP || event.actionMasked == MotionEvent.ACTION_POINTER_UP
        return if (finishing && !cancelled && actionPointer in accumulator.pendingPointers()) listOf(accumulator.complete(actionPointer)) else emptyList()
    }
}
