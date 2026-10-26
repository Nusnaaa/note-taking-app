package org.stemnotebook.backend

import android.content.ContextWrapper
import android.view.InputDevice
import android.view.MotionEvent
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import java.io.File
import java.util.UUID
import java.util.concurrent.ExecutionException
import java.util.concurrent.TimeUnit

@RunWith(JniBackendRunner::class)
@Config(sdk = [35])
class JournaledCaptureIntegrationTest {
    @get:Rule val temporary = TemporaryFolder()
    private fun platform(root: File) = object : ContextWrapper(RuntimeEnvironment.getApplication()) { override fun getNoBackupFilesDir(): File = root }
    private fun call(backend: NotebookBackend, command: JSONObject): JSONObject {
        val response = JSONObject(backend.request(JSONObject().put("version", 1).put("command", command).toString()).get(5, TimeUnit.SECONDS))
        assertTrue(response.optString("error"), response.getBoolean("ok")); return response
    }
    private fun note(backend: NotebookBackend) = UUID.fromString(call(backend, JSONObject().put("op", "Create").put("kind", "Note").put("parent", JSONObject.NULL).put("title", "capture journal fixture")).getJSONObject("data").getString("id"))
    private fun blocks(backend: NotebookBackend, entity: UUID) = call(backend, JSONObject().put("op", "Blocks").put("entity", entity.toString()).put("offset", 0).put("limit", 100)).getJSONArray("data")
    private fun event(action: Int, time: Long): MotionEvent {
        val property = MotionEvent.PointerProperties().apply { id = 7; toolType = MotionEvent.TOOL_TYPE_STYLUS }
        val coordinate = MotionEvent.PointerCoords().apply { x = 0.125f; y = 3.5f; pressure = 0.5f }
        return MotionEvent.obtain(10, time, action, 1, arrayOf(property), arrayOf(coordinate), 0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_STYLUS, 0)
    }
    private fun submit(session: JournaledCaptureSession, action: Int, time: Long): JournaledCaptureSession.Submission = event(action, time).let { try { session.accept(it).single() } finally { it.recycle() } }
    @Test(timeout = 30_000) fun explicitDiscardRetiresCallerHeldFramesEvenAfterSessionInterruption() {
        for (interrupted in listOf(false, true)) for (combined in listOf(false, true)) {
            val backend = NotebookBackend(platform(temporary.newFolder()))
            try {
                val canvas = UUID.fromString(call(backend, JSONObject().put("op", "Create").put("kind", "Canvas").put("parent", JSONObject.NULL).put("title", "discard retirement")).getJSONObject("data").getString("id"))
                val session = backend.openCaptureSession(canvas, "fixed-space", layer = CaptureLayer.Custom(UUID.randomUUID()))
                submit(session, MotionEvent.ACTION_DOWN, 10).receipt.get(5, TimeUnit.SECONDS)
                val pending = if (interrupted) session.interrupt().single() else submit(session, MotionEvent.ACTION_UP, 20)
                assertFalse(pending.receipt.get(5, TimeUnit.SECONDS).committed)
                val review = backend.reviewInterruptedCapture(UUID.fromString(pending.stroke.id)).get(5, TimeUnit.SECONDS)
                if (combined) {
                    val deletion = backend.prepareDeletionWithCaptures(canvas, false).get(5, TimeUnit.SECONDS)
                    val receipt = backend.confirmDeletionWithCaptures(deletion, CaptureDeletionDisposition.DiscardReviewedSealed, true).get(5, TimeUnit.SECONDS)
                    assertEquals(1, receipt.draftsRemoved)
                    val entry = call(backend, JSONObject().put("op", "TrashEntries").put("offset", 0).put("limit", 10)).getJSONArray("data").getJSONObject(0)
                    call(backend, JSONObject().put("op", "RestoreEntity").put("entity", canvas.toString()).put("revision", entry.getLong("revision")))
                } else backend.discardInterruptedCapture(review, true).get(5, TimeUnit.SECONDS)
                assertEquals(0, backend.pendingCaptures().get(5, TimeUnit.SECONDS).toInt())
                assertThrows(ExecutionException::class.java) { session.retry(pending).receipt.get(5, TimeUnit.SECONDS) }
                assertEquals(0, backend.pendingCaptures().get(5, TimeUnit.SECONDS).toInt()); assertEquals(0, blocks(backend, canvas).length())
            } finally { backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
        }
    }
    @Test(timeout = 20_000) fun unacknowledgedDiscardAfterUnlinkCannotResurrectAndKeepsOtherFramesRetryable() {
        val armed = java.util.concurrent.atomic.AtomicBoolean(false)
        val backend = NotebookBackend(platform(temporary.newFolder()), { platform, native -> PdfWorkspace(platform, native) }, captureJournalFault = { if (it == "after-delete" && armed.getAndSet(false)) throw java.io.IOException("synthetic directory acknowledgement failure") })
        try {
            val canvas = UUID.fromString(call(backend, JSONObject().put("op", "Create").put("kind", "Canvas").put("parent", JSONObject.NULL).put("title", "discard failure")).getJSONObject("data").getString("id"))
            val session = backend.openCaptureSession(canvas, "fixed-space", layer = CaptureLayer.Custom(UUID.randomUUID()))
            submit(session, MotionEvent.ACTION_DOWN, 10).receipt.get(5, TimeUnit.SECONDS)
            val first = submit(session, MotionEvent.ACTION_UP, 20); first.receipt.get(5, TimeUnit.SECONDS)
            submit(session, MotionEvent.ACTION_DOWN, 30).receipt.get(5, TimeUnit.SECONDS)
            val second = submit(session, MotionEvent.ACTION_UP, 40); second.receipt.get(5, TimeUnit.SECONDS)
            val review = backend.reviewInterruptedCapture(UUID.fromString(first.stroke.id)).get(5, TimeUnit.SECONDS)
            armed.set(true)
            assertThrows(ExecutionException::class.java) { backend.discardInterruptedCapture(review, true).get(5, TimeUnit.SECONDS) }
            assertEquals(1, backend.pendingCaptures().get(5, TimeUnit.SECONDS).toInt())
            assertThrows(ExecutionException::class.java) { session.retry(first).receipt.get(5, TimeUnit.SECONDS) }
            assertFalse(session.retry(second).receipt.get(5, TimeUnit.SECONDS).committed)
            assertEquals(second.stroke, backend.reviewInterruptedCapture(UUID.fromString(second.stroke.id)).get(5, TimeUnit.SECONDS).stroke)
            assertEquals(1, backend.pendingCaptures().get(5, TimeUnit.SECONDS).toInt()); assertEquals(0, blocks(backend, canvas).length())
        } finally { backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
    @Test(timeout = 30_000) fun fixedLayerCaptureAndMissingDestinationRecoveryNeverFallbackOrRewriteObservations() {
        val root = temporary.newFolder(); val context = platform(root); var backend = NotebookBackend(context)
        try {
            val canvas = UUID.fromString(call(backend, JSONObject().put("op", "Create").put("kind", "Canvas").put("parent", JSONObject.NULL).put("title", "layer capture")).getJSONObject("data").getString("id"))
            val layer = CaptureLayer.Custom(UUID.randomUUID())
            val state = JSONObject().put("canvas", canvas.toString()).put("revision", 0).put("base_visible", false).put("layers", org.json.JSONArray().put(JSONObject().put("id", layer.id.toString()).put("name", "ink").put("visible", true).put("blocks", org.json.JSONArray())))
            call(backend, JSONObject().put("op", "SetCanvasLayers").put("state", state).put("expected", 0))
            val style = StrokeStyle(17, 128, 255, 64, 0.6)
            val session = backend.openCaptureSession(canvas, "fixed-space", style, layer)
            submit(session, MotionEvent.ACTION_DOWN, 10).receipt.get(5, TimeUnit.SECONDS)
            val first = submit(session, MotionEvent.ACTION_UP, 20); assertTrue(first.receipt.get(5, TimeUnit.SECONDS).committed)
            assertEquals(first.stroke.id, call(backend, JSONObject().put("op", "SourceInk").put("block", blocks(backend, canvas).getJSONObject(0).getString("id"))).getJSONArray("data").getJSONObject(0).getString("id"))
            val saved = call(backend, JSONObject().put("op", "CanvasLayers").put("canvas", canvas.toString())).getJSONObject("data")
            assertEquals(1, saved.getJSONArray("layers").getJSONObject(0).getJSONArray("blocks").length())
            val revision = saved.getLong("revision"); saved.put("layers", org.json.JSONArray())
            call(backend, JSONObject().put("op", "SetCanvasLayers").put("state", saved).put("expected", revision))
            assertTrue(session.retry(first).receipt.get(5, TimeUnit.SECONDS).committed)
            submit(session, MotionEvent.ACTION_DOWN, 30).receipt.get(5, TimeUnit.SECONDS)
            val blocked = submit(session, MotionEvent.ACTION_UP, 40); assertFalse(blocked.receipt.get(5, TimeUnit.SECONDS).committed)
            assertEquals(1, blocks(backend, canvas).length())
            backend.closeFullyAsync().get(5, TimeUnit.SECONDS); backend = NotebookBackend(context)
            val review = backend.reviewInterruptedCapture(UUID.fromString(blocked.stroke.id)).get(5, TimeUnit.SECONDS)
            assertEquals(layer, review.layer); assertEquals(false, review.layerAvailable); assertEquals(style, review.stroke.style); assertEquals(blocked.stroke.points, review.stroke.points)
            assertThrows(ExecutionException::class.java) { backend.acceptInterruptedCapture(review, true).get(5, TimeUnit.SECONDS) }
            assertEquals(1, backend.pendingCaptures().get(5, TimeUnit.SECONDS).toInt()); assertEquals(1, blocks(backend, canvas).length())
            val blockId = blocks(backend, canvas).getJSONObject(0).getString("id")
            backend.authenticate("[{\"Block\":\"$blockId\"}]") { true }.get(5, TimeUnit.SECONDS)
            call(backend, JSONObject().put("op", "SetBlockLock").put("block", blockId).put("revision", 0).put("locked", true))
            call(backend, JSONObject().put("op", "Relock"))
            val protectedReview = backend.reviewInterruptedCapture(UUID.fromString(blocked.stroke.id)).get(5, TimeUnit.SECONDS)
            assertNull(protectedReview.layerAvailable); assertEquals(blocked.stroke.points, protectedReview.stroke.points)
            assertThrows(ExecutionException::class.java) { backend.acceptInterruptedCapture(protectedReview, true).get(5, TimeUnit.SECONDS) }
            backend.discardInterruptedCapture(protectedReview, true).get(5, TimeUnit.SECONDS)
        } finally { backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
    @Test(timeout = 30_000) fun explicitRetargetChecksLayerRevisionFreshAuthorityAndOriginalTicketBeforePublishing() {
        val root = temporary.newFolder(); val context = platform(root); var backend = NotebookBackend(context)
        try {
            val canvas = UUID.fromString(call(backend, JSONObject().put("op", "Create").put("kind", "Canvas").put("parent", JSONObject.NULL).put("title", "retarget")).getJSONObject("data").getString("id"))
            val missing = CaptureLayer.Custom(UUID.randomUUID()); val chosen = StrokeStyle(17, 128, 255, 64, 0.6)
            val session = backend.openCaptureSession(canvas, "fixed-space", chosen, missing)
            submit(session, MotionEvent.ACTION_DOWN, 10).receipt.get(5, TimeUnit.SECONDS)
            val pending = submit(session, MotionEvent.ACTION_UP, 20); assertFalse(pending.receipt.get(5, TimeUnit.SECONDS).committed)
            val ticket = UUID.fromString(pending.stroke.id); val source = backend.reviewInterruptedCapture(ticket).get(5, TimeUnit.SECONDS)
            assertThrows(ExecutionException::class.java) { backend.reviewCaptureRetarget(source, CaptureLayer.Custom(UUID.randomUUID())).get(5, TimeUnit.SECONDS) }
            val stale = backend.reviewCaptureRetarget(source, CaptureLayer.Base).get(5, TimeUnit.SECONDS)
            val layers = call(backend, JSONObject().put("op", "CanvasLayers").put("canvas", canvas.toString())).getJSONObject("data"); val revision = layers.getLong("revision")
            layers.put("base_visible", false)
            call(backend, JSONObject().put("op", "SetCanvasLayers").put("state", layers).put("expected", revision))
            assertThrows(ExecutionException::class.java) { backend.confirmCaptureRetarget(stale, true).get(5, TimeUnit.SECONDS) }
            assertEquals(source.checksum, backend.reviewInterruptedCapture(ticket).get(5, TimeUnit.SECONDS).checksum)
            val reviewed = backend.reviewCaptureRetarget(source, CaptureLayer.Base).get(5, TimeUnit.SECONDS)
            assertThrows(IllegalArgumentException::class.java) { backend.confirmCaptureRetarget(reviewed, false) }
            val scope = "[{\"Entity\":\"$canvas\"}]"
            backend.authenticate(scope) { true }.get(5, TimeUnit.SECONDS)
            val entityRevision = call(backend, JSONObject().put("op", "Read").put("id", canvas.toString())).getJSONObject("data").getLong("revision")
            call(backend, JSONObject().put("op", "SetLock").put("entity", canvas.toString()).put("revision", entityRevision).put("locked", true))
            call(backend, JSONObject().put("op", "Relock"))
            assertThrows(ExecutionException::class.java) { backend.confirmCaptureRetarget(reviewed, true).get(5, TimeUnit.SECONDS) }
            backend.authenticate(scope) { true }.get(5, TimeUnit.SECONDS)
            val receipt = backend.confirmCaptureRetarget(reviewed, true).get(5, TimeUnit.SECONDS)
            assertTrue(receipt.checkpointed); assertTrue(receipt.committed); assertEquals(ticket, receipt.ticket); assertEquals(CaptureLayer.Base, receipt.destination)
            assertEquals(0, backend.pendingCaptures().get(5, TimeUnit.SECONDS).toInt()); assertEquals(1, blocks(backend, canvas).length())
            val block = blocks(backend, canvas).getJSONObject(0).getString("id")
            val ink = call(backend, JSONObject().put("op", "SourceInk").put("block", block)).getJSONArray("data").getJSONObject(0)
            assertEquals(pending.stroke.id, ink.getString("id")); assertEquals(pending.stroke.session, ink.getString("capture_session")); assertEquals(2, ink.getJSONArray("points").length())
            assertEquals(0.125, ink.getJSONArray("points").getJSONObject(0).getDouble("x"), 0.0); assertEquals(chosen.width, ink.getJSONObject("style").getDouble("width"), 0.0)
            // A caller still holding the old immutable frame cannot republish the old destination.
            assertThrows(ExecutionException::class.java) { session.retry(pending).receipt.get(5, TimeUnit.SECONDS) }
            assertEquals(0, backend.pendingCaptures().get(5, TimeUnit.SECONDS).toInt()); assertEquals(1, blocks(backend, canvas).length())
            assertThrows(ExecutionException::class.java) { backend.confirmCaptureRetarget(reviewed, true).get(5, TimeUnit.SECONDS) }
            backend.closeFullyAsync().get(5, TimeUnit.SECONDS); backend = NotebookBackend(context)
            backend.authenticate(scope) { true }.get(5, TimeUnit.SECONDS)
            assertEquals(1, blocks(backend, canvas).length()); assertEquals(0, backend.pendingCaptures().get(5, TimeUnit.SECONDS).toInt())
        } finally { backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
    @Test(timeout = 20_000) fun ambiguousNativeCommitCannotBeRetargetedOrDuplicated() {
        val armed = java.util.concurrent.atomic.AtomicBoolean(false)
        val backend = NotebookBackend(platform(temporary.newFolder()), { platform, native -> PdfWorkspace(platform, native) }, captureJournalFault = { if (it == "after-native-reply" && armed.getAndSet(false)) throw java.io.IOException("synthetic acknowledgement loss") })
        try {
            val canvas = UUID.fromString(call(backend, JSONObject().put("op", "Create").put("kind", "Canvas").put("parent", JSONObject.NULL).put("title", "ambiguous retarget")).getJSONObject("data").getString("id"))
            val session = backend.openCaptureSession(canvas, "fixed-space", layer = CaptureLayer.Base)
            submit(session, MotionEvent.ACTION_DOWN, 10).receipt.get(5, TimeUnit.SECONDS); armed.set(true)
            val pending = submit(session, MotionEvent.ACTION_UP, 20)
            assertThrows(ExecutionException::class.java) { pending.receipt.get(5, TimeUnit.SECONDS) }
            assertEquals(1, blocks(backend, canvas).length()); assertEquals(1, backend.pendingCaptures().get(5, TimeUnit.SECONDS).toInt())
            val review = backend.reviewInterruptedCapture(UUID.fromString(pending.stroke.id)).get(5, TimeUnit.SECONDS)
            val error = assertThrows(ExecutionException::class.java) { backend.reviewCaptureRetarget(review, CaptureLayer.Custom(UUID.randomUUID())).get(5, TimeUnit.SECONDS) }
            assertEquals("Capture already committed", error.cause!!.message)
            assertEquals(review.checksum, backend.reviewInterruptedCapture(review.ticket).get(5, TimeUnit.SECONDS).checksum)
            backend.retryCaptures().get(5, TimeUnit.SECONDS)
            assertEquals(1, blocks(backend, canvas).length()); assertEquals(0, backend.pendingCaptures().get(5, TimeUnit.SECONDS).toInt())
        } finally { backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
    @Test(timeout = 30_000) fun reopenedInterruptedDraftRetargetsToChosenCustomLayerWithExactSourceIdentity() {
        val root = temporary.newFolder(); val context = platform(root); var backend = NotebookBackend(context)
        try {
            val canvas = UUID.fromString(call(backend, JSONObject().put("op", "Create").put("kind", "Canvas").put("parent", JSONObject.NULL).put("title", "custom retarget")).getJSONObject("data").getString("id"))
            val selected = CaptureLayer.Custom(UUID.randomUUID())
            val state = JSONObject().put("canvas", canvas.toString()).put("revision", 0).put("base_visible", true).put("layers", org.json.JSONArray().put(JSONObject().put("id", selected.id.toString()).put("name", "chosen").put("visible", true).put("blocks", org.json.JSONArray())))
            call(backend, JSONObject().put("op", "SetCanvasLayers").put("state", state).put("expected", 0))
            val style = StrokeStyle(17, 128, 255, 64, 0.6)
            val session = backend.openCaptureSession(canvas, "fixed-space", style, CaptureLayer.Custom(UUID.randomUUID()))
            submit(session, MotionEvent.ACTION_DOWN, 10).receipt.get(5, TimeUnit.SECONDS); submit(session, MotionEvent.ACTION_MOVE, 20).receipt.get(5, TimeUnit.SECONDS)
            val pending = session.interrupt().single(); assertFalse(pending.receipt.get(5, TimeUnit.SECONDS).committed)
            backend.closeFullyAsync().get(5, TimeUnit.SECONDS); backend = NotebookBackend(context)
            val source = backend.reviewInterruptedCapture(UUID.fromString(pending.stroke.id)).get(5, TimeUnit.SECONDS)
            assertEquals(pending.stroke, source.stroke)
            val review = backend.reviewCaptureRetarget(source, selected).get(5, TimeUnit.SECONDS)
            assertTrue(backend.confirmCaptureRetarget(review, true).get(5, TimeUnit.SECONDS).committed)
            val block = blocks(backend, canvas).getJSONObject(0).getString("id")
            val layers = call(backend, JSONObject().put("op", "CanvasLayers").put("canvas", canvas.toString())).getJSONObject("data")
            assertEquals(block, layers.getJSONArray("layers").getJSONObject(0).getJSONArray("blocks").getString(0))
            val ink = call(backend, JSONObject().put("op", "SourceInk").put("block", block)).getJSONArray("data").getJSONObject(0)
            assertEquals(pending.stroke.id, ink.getString("id")); assertEquals(pending.stroke.session, ink.getString("capture_session"))
            assertEquals("Lifecycle", ink.getJSONObject("end").getString("Interrupted")); assertEquals(2, ink.getJSONArray("points").length()); assertEquals(style.width, ink.getJSONObject("style").getDouble("width"), 0.0)
            assertEquals(0, backend.pendingCaptures().get(5, TimeUnit.SECONDS).toInt())
        } finally { backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
    @Test(timeout = 20_000) fun conflictingUnclassifiedLegacyEnvelopeCannotBeBypassedByRetarget() {
        val root = temporary.newFolder(); val context = platform(root); var backend = NotebookBackend(context)
        try {
            val canvas = UUID.fromString(call(backend, JSONObject().put("op", "Create").put("kind", "Canvas").put("parent", JSONObject.NULL).put("title", "legacy conflict")).getJSONObject("data").getString("id"))
            val session = backend.openCaptureSession(canvas, "fixed-space", layer = CaptureLayer.Custom(UUID.randomUUID()))
            submit(session, MotionEvent.ACTION_DOWN, 10).receipt.get(5, TimeUnit.SECONDS)
            val pending = session.interrupt().single(); pending.receipt.get(5, TimeUnit.SECONDS)
            backend.closeFullyAsync().get(5, TimeUnit.SECONDS)
            val ticket = UUID.fromString(pending.stroke.id)
            // Simulate an older/unclassified envelope while the library is fully closed.
            CaptureOutbox(File(root, "notebook/capture-recovery")).enqueue(ticket, "{\"version\":1,\"command\":{\"op\":\"UnknownOldCapture\"}}")
            backend = NotebookBackend(context)
            val source = backend.reviewInterruptedCapture(ticket).get(5, TimeUnit.SECONDS)
            val error = assertThrows(ExecutionException::class.java) { backend.reviewCaptureRetarget(source, CaptureLayer.Base).get(5, TimeUnit.SECONDS) }
            assertEquals("Conflicting legacy capture", error.cause!!.message)
            assertEquals(source.checksum, backend.reviewInterruptedCapture(ticket).get(5, TimeUnit.SECONDS).checksum)
            assertEquals(0, blocks(backend, canvas).length()); assertEquals(2, backend.pendingCaptures().get(5, TimeUnit.SECONDS).toInt())
        } finally { backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
    @Test(timeout = 30_000) fun actualJniRetainsPerContactPressureResponseAcrossConfigurationChangeAndRetry() {
        val backend = NotebookBackend(platform(temporary.newFolder()))
        try {
            val entity = note(backend); val response = PressureResponse(0.0, 2.0, 0.25, 1.25, 2.0)
            val chosen = StrokeStyle(32, 64, 128, 255, 8.0, response); val session = backend.openCaptureSession(entity, "fixed-space", chosen)
            submit(session, MotionEvent.ACTION_DOWN, 10).receipt.get(5, TimeUnit.SECONDS)
            session.setStyle(chosen.copy(pressureResponse = null))
            val first = submit(session, MotionEvent.ACTION_UP, 20); assertTrue(first.receipt.get(5, TimeUnit.SECONDS).committed)
            assertTrue(session.retry(first).receipt.get(5, TimeUnit.SECONDS).committed)
            submit(session, MotionEvent.ACTION_DOWN, 30).receipt.get(5, TimeUnit.SECONDS)
            assertTrue(submit(session, MotionEvent.ACTION_UP, 40).receipt.get(5, TimeUnit.SECONDS).committed)
            val rows = blocks(backend, entity); assertEquals(2, rows.length())
            val sources = (0 until rows.length()).map { index -> call(backend, JSONObject().put("op", "SourceInk").put("block", rows.getJSONObject(index).getString("id"))).getJSONArray("data").getJSONObject(0) }
            assertEquals(1, sources.count { it.getJSONObject("style").has("pressure_response") })
            val persisted = sources.single { it.getJSONObject("style").has("pressure_response") }.getJSONObject("style").getJSONObject("pressure_response")
            assertEquals(2.0, persisted.getDouble("input_max"), 0.0); assertEquals(2.0, persisted.getDouble("exponent"), 0.0)
            assertEquals(chosen, first.stroke.style); assertEquals(2, first.stroke.points.size)
        } finally { backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
    @Test(timeout = 20_000) fun actualJniRetainsPerContactStyleAcrossConfigurationChangeAndExactRetry() {
        val backend = NotebookBackend(platform(temporary.newFolder()))
        try {
            val entity = note(backend); val first = StrokeStyle(17, 128, 255, 64, 0.12345678901234567); val second = first.copy(width = 8.125, red = 200)
            val session = backend.openCaptureSession(entity, "fixed-space", first)
            submit(session, MotionEvent.ACTION_DOWN, 10).receipt.get(5, TimeUnit.SECONDS); session.setStyle(second)
            val up = submit(session, MotionEvent.ACTION_UP, 20); assertTrue(up.receipt.get(5, TimeUnit.SECONDS).committed); assertTrue(session.retry(up).receipt.get(5, TimeUnit.SECONDS).committed)
            submit(session, MotionEvent.ACTION_DOWN, 30).receipt.get(5, TimeUnit.SECONDS); submit(session, MotionEvent.ACTION_UP, 40).receipt.get(5, TimeUnit.SECONDS)
            val rows = blocks(backend, entity); assertEquals(2, rows.length())
            for ((index, expected) in listOf(first, second).withIndex()) {
                val source = call(backend, JSONObject().put("op", "SourceInk").put("block", rows.getJSONObject(index).getString("id"))).getJSONArray("data").getJSONObject(0)
                assertEquals(expected.width, source.getJSONObject("style").getDouble("width"), 0.0); assertEquals(expected.red, source.getJSONObject("style").getJSONArray("colour_rgba").getInt(0)); assertEquals(2, source.getJSONArray("points").length())
            }
        } finally { backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
    @Test(timeout = 20_000) fun checkpointIsNotDocumentCommitAndCompletionRetiresJournalWithExactTicketRetry() {
        val root = temporary.newFolder(); val backend = NotebookBackend(platform(root))
        try {
            val entity = note(backend); val session = backend.openCaptureSession(entity, "fixed-space")
            val down = submit(session, MotionEvent.ACTION_DOWN, 10); val saved = down.receipt.get(5, TimeUnit.SECONDS)
            assertTrue(saved.checkpointed); assertFalse(saved.committed); assertEquals(0, blocks(backend, entity).length())
            val move = submit(session, MotionEvent.ACTION_MOVE, 20); move.receipt.get(5, TimeUnit.SECONDS)
            val up = submit(session, MotionEvent.ACTION_UP, 30); assertTrue(up.receipt.get(5, TimeUnit.SECONDS).committed)
            assertEquals(down.stroke.id, up.stroke.id); assertEquals(1, blocks(backend, entity).length())
            assertTrue(session.retry(up).receipt.get(5, TimeUnit.SECONDS).committed)
            assertThrows(ExecutionException::class.java) { session.retry(down).receipt.get(5, TimeUnit.SECONDS) }
            assertEquals(1, blocks(backend, entity).length()); assertEquals(0, backend.captureRecoveryState().get(5, TimeUnit.SECONDS).pending)
        } finally { backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
    @Test(timeout = 20_000) fun platformCancellationDoesNotResurrectFromOldFramesOrReopen() {
        val root = temporary.newFolder(); val context = platform(root); val backend = NotebookBackend(context)
        val entity = note(backend)
        try {
            val session = backend.openCaptureSession(entity, "fixed-space"); val down = submit(session, MotionEvent.ACTION_DOWN, 10); down.receipt.get(5, TimeUnit.SECONDS)
            val cancelled = submit(session, MotionEvent.ACTION_CANCEL, 20); assertTrue(cancelled.cancelled); assertTrue(cancelled.receipt.get(5, TimeUnit.SECONDS).discarded)
            assertTrue(session.retry(cancelled).receipt.get(5, TimeUnit.SECONDS).discarded)
            assertThrows(ExecutionException::class.java) { session.retry(down).receipt.get(5, TimeUnit.SECONDS) }
            assertEquals(0, blocks(backend, entity).length())
        } finally { backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
        val reopened = NotebookBackend(context)
        try { assertEquals(0, reopened.pendingCaptures().get(5, TimeUnit.SECONDS).toInt()); assertEquals(0, blocks(reopened, entity).length()) } finally { reopened.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
    @Test(timeout = 20_000) fun gracefulCloseSealsLatestPrefixWithoutInventingPenUp() {
        val root = temporary.newFolder(); val context = platform(root); val backend = NotebookBackend(context); val entity = note(backend)
        val session = backend.openCaptureSession(entity, "fixed-space")
        submit(session, MotionEvent.ACTION_DOWN, 10).receipt.get(5, TimeUnit.SECONDS); submit(session, MotionEvent.ACTION_MOVE, 20).receipt.get(5, TimeUnit.SECONDS)
        backend.closeFullyAsync().get(5, TimeUnit.SECONDS)
        assertEquals(2, session.interruptedObservations().single().points.size)
        val reopened = NotebookBackend(context)
        try {
            val rows = blocks(reopened, entity); assertEquals(1, rows.length())
            val source = call(reopened, JSONObject().put("op", "SourceInk").put("block", rows.getJSONObject(0).getString("id"))).getJSONArray("data").getJSONObject(0)
            assertEquals("Lifecycle", source.getJSONObject("end").getString("Interrupted")); assertEquals(2, source.getJSONArray("points").length())
            assertEquals(10_000L, source.getJSONArray("points").getJSONObject(1).getLong("time_us"))
        } finally { reopened.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
    @Test(timeout = 20_000) fun processRecoveryRequiresReviewAndFreshPermissionAndPreservesExactObservedBits() {
        val root = temporary.newFolder(); val context = platform(root); val seed = NotebookBackend(context); val entity = note(seed)
        seed.closeFullyAsync().get(5, TimeUnit.SECONDS)
        val handle = NativeBridge.open(File(root, "notebook/library.db").absolutePath)
        val identity = try { UUID.fromString(NativeBridge.captureLibraryIdentity(handle)) } finally { NativeBridge.close(handle) }
        val original = CapturedStroke(UUID.randomUUID().toString(), UUID.randomUUID().toString(), 0, 7, false, listOf(ObservedPoint(-0.0, 1.2345678901234567, 10, 0.0, null, null, 0)), StrokeEnd.InProgress)
        val ticket = UUID.fromString(original.id)
        ActiveCaptureJournal(File(root, "notebook/active-capture"), identity).checkpoint(entity, "fixed-space", 0, original)
        val backend = NotebookBackend(context)
        try {
            assertEquals(1, backend.pendingCaptures().get(5, TimeUnit.SECONDS).toInt()); assertEquals(0, blocks(backend, entity).length())
            assertEquals(listOf(ticket), backend.interruptedCaptureTickets().get(5, TimeUnit.SECONDS))
            val review = backend.reviewInterruptedCapture(ticket).get(5, TimeUnit.SECONDS)
            assertEquals(StrokeEnd.Interrupted(StrokeInterruption.Recovery), review.stroke.end); assertEquals(original.points, review.stroke.points)
            assertThrows(IllegalArgumentException::class.java) { backend.acceptInterruptedCapture(review, false) }
            backend.acceptInterruptedCapture(review, true).get(5, TimeUnit.SECONDS)
            assertThrows(ExecutionException::class.java) { backend.acceptInterruptedCapture(review, true).get(5, TimeUnit.SECONDS) }
            val rows = blocks(backend, entity); assertEquals(1, rows.length())
            val source = call(backend, JSONObject().put("op", "SourceInk").put("block", rows.getJSONObject(0).getString("id"))).getJSONArray("data").getJSONObject(0)
            assertEquals("Recovery", source.getJSONObject("end").getString("Interrupted")); assertEquals((-0.0).toRawBits(), source.getJSONArray("points").getJSONObject(0).getDouble("x").toRawBits())
        } finally { backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
    @Test(timeout = 30_000) fun protectedRecoveryNeedsFreshPermissionAndDoesNotBlockIndependentWriting() {
        val root = temporary.newFolder(); val context = platform(root); val seed = NotebookBackend(context); val entity = note(seed)
        val scope = "[{\"Entity\":\"$entity\"}]"
        seed.authenticate(scope) { true }.get(5, TimeUnit.SECONDS)
        call(seed, JSONObject().put("op", "SetLock").put("entity", entity.toString()).put("revision", 0).put("locked", true))
        seed.closeFullyAsync().get(5, TimeUnit.SECONDS)
        val handle = NativeBridge.open(File(root, "notebook/library.db").absolutePath)
        val identity = try { UUID.fromString(NativeBridge.captureLibraryIdentity(handle)) } finally { NativeBridge.close(handle) }
        val source = CapturedStroke(UUID.randomUUID().toString(), UUID.randomUUID().toString(), 0, 7, false, listOf(ObservedPoint(-0.0, 2.0, 10, null, null, null, null)), StrokeEnd.InProgress)
        val ticket = UUID.fromString(source.id)
        ActiveCaptureJournal(File(root, "notebook/active-capture"), identity).checkpoint(entity, "fixed-space", 0, source)
        val backend = NotebookBackend(context)
        try {
            assertTrue(backend.interruptedCaptureTickets().get(5, TimeUnit.SECONDS).isEmpty())
            assertThrows(ExecutionException::class.java) { backend.reviewInterruptedCapture(ticket).get(5, TimeUnit.SECONDS) }
            val independent = note(backend); val session = backend.openCaptureSession(independent, "independent-space")
            submit(session, MotionEvent.ACTION_DOWN, 10).receipt.get(5, TimeUnit.SECONDS)
            assertTrue(submit(session, MotionEvent.ACTION_UP, 20).receipt.get(5, TimeUnit.SECONDS).committed)
            assertEquals(1, blocks(backend, independent).length())
            backend.authenticate(scope) { true }.get(5, TimeUnit.SECONDS)
            val review = backend.reviewInterruptedCapture(ticket).get(5, TimeUnit.SECONDS)
            call(backend, JSONObject().put("op", "Relock"))
            assertThrows(ExecutionException::class.java) { backend.acceptInterruptedCapture(review, true).get(5, TimeUnit.SECONDS) }
            backend.authenticate(scope) { true }.get(5, TimeUnit.SECONDS)
            backend.acceptInterruptedCapture(review, true).get(5, TimeUnit.SECONDS)
            assertEquals(1, blocks(backend, entity).length()); assertEquals(0, backend.pendingCaptures().get(5, TimeUnit.SECONDS).toInt())
        } finally { backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
    @Test(timeout = 20_000) fun explicitDiscardRemovesOnlyReviewedUncommittedRecoveryEvidence() {
        val root = temporary.newFolder(); val context = platform(root); val seed = NotebookBackend(context); val entity = note(seed)
        seed.closeFullyAsync().get(5, TimeUnit.SECONDS)
        val handle = NativeBridge.open(File(root, "notebook/library.db").absolutePath)
        val identity = try { UUID.fromString(NativeBridge.captureLibraryIdentity(handle)) } finally { NativeBridge.close(handle) }
        val source = CapturedStroke(UUID.randomUUID().toString(), UUID.randomUUID().toString(), 0, 7, false, listOf(ObservedPoint(1.0, 2.0, 10, null, null, null, null)), StrokeEnd.InProgress)
        val ticket = UUID.fromString(source.id)
        ActiveCaptureJournal(File(root, "notebook/active-capture"), identity).checkpoint(entity, "fixed-space", 0, source)
        val backend = NotebookBackend(context)
        try {
            val review = backend.reviewInterruptedCapture(ticket).get(5, TimeUnit.SECONDS)
            assertThrows(IllegalArgumentException::class.java) { backend.discardInterruptedCapture(review, false) }
            backend.discardInterruptedCapture(review, true).get(5, TimeUnit.SECONDS)
            assertEquals(0, backend.pendingCaptures().get(5, TimeUnit.SECONDS).toInt()); assertEquals(0, blocks(backend, entity).length())
            assertThrows(ExecutionException::class.java) { backend.acceptInterruptedCapture(review, true).get(5, TimeUnit.SECONDS) }
        } finally { backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }

    @Test(timeout = 30_000)
    fun retiredTargetsRequireFreshScopedReviewAndPermitDiscardWithoutReplay() {
        for (purged in listOf(false, true)) {
            val root = temporary.newFolder(); val context = platform(root); val seed = NotebookBackend(context); val entity = note(seed)
            val trash = call(seed, JSONObject().put("op", "PrepareDeletion").put("entity", entity.toString()).put("permanent", false)).getJSONObject("data").getString("review")
            call(seed, JSONObject().put("op", "ConfirmDeletion").put("review", trash).put("confirmed", true))
            if (purged) {
                val purge = call(seed, JSONObject().put("op", "PrepareDeletion").put("entity", entity.toString()).put("permanent", true)).getJSONObject("data").getString("review")
                call(seed, JSONObject().put("op", "ConfirmDeletion").put("review", purge).put("confirmed", true))
            }
            seed.closeFullyAsync().get(5, TimeUnit.SECONDS)
            val database = File(root, "notebook/library.db")
            val handle = NativeBridge.open(database.absolutePath)
            val identity = try { UUID.fromString(NativeBridge.captureLibraryIdentity(handle)) } finally { NativeBridge.close(handle) }
            val source = CapturedStroke(UUID.randomUUID().toString(), UUID.randomUUID().toString(), 0, 7, false, listOf(ObservedPoint(-0.0, 2.0, 10, null, null, null, null)), StrokeEnd.InProgress)
            val ticket = UUID.fromString(source.id)
            ActiveCaptureJournal(File(root, "notebook/active-capture"), identity).checkpoint(entity, "fixed-space", 0, source)
            val backend = NotebookBackend(context)
            try {
                assertTrue(backend.interruptedCaptureTickets().get(5, TimeUnit.SECONDS).isEmpty())
                assertThrows(ExecutionException::class.java) { backend.reviewInterruptedCapture(ticket).get(5, TimeUnit.SECONDS) }
                val scope = "[{\"Entity\":\"$entity\"}]"
                backend.authenticate(scope) { true }.get(5, TimeUnit.SECONDS)
                assertEquals(listOf(ticket), backend.interruptedCaptureTickets().get(5, TimeUnit.SECONDS))
                val review = backend.reviewInterruptedCapture(ticket).get(5, TimeUnit.SECONDS)
                assertEquals(if (purged) NotebookBackend.CaptureTargetState.Purged else NotebookBackend.CaptureTargetState.Trashed, review.targetState)
                assertEquals(source.points, review.stroke.points)
                assertThrows(ExecutionException::class.java) { backend.reviewCaptureRetarget(review, CaptureLayer.Base).get(5, TimeUnit.SECONDS) }
                assertThrows(ExecutionException::class.java) { backend.acceptInterruptedCapture(review, true).get(5, TimeUnit.SECONDS) }
                assertEquals(1, backend.pendingCaptures().get(5, TimeUnit.SECONDS).toInt())
                call(backend, JSONObject().put("op", "Relock"))
                assertThrows(ExecutionException::class.java) { backend.discardInterruptedCapture(review, true).get(5, TimeUnit.SECONDS) }
                backend.authenticate(scope) { true }.get(5, TimeUnit.SECONDS)
                backend.discardInterruptedCapture(review, true).get(5, TimeUnit.SECONDS)
                assertEquals(0, backend.pendingCaptures().get(5, TimeUnit.SECONDS).toInt())
            } finally { backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
        }
    }

    @Test(timeout = 20_000) fun cancellingObservationFuturesCannotDropAdmittedCheckpointOrLegacyInk() {
        val root = temporary.newFolder(); val backend = NotebookBackend(platform(root)); val entity = note(backend)
        val entered = java.util.concurrent.CountDownLatch(1); val release = java.util.concurrent.CountDownLatch(1)
        try {
            val authentication = backend.authenticate("[\"App\"]") { entered.countDown(); release.await(10, TimeUnit.SECONDS) }
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            val session = backend.openCaptureSession(entity, "fixed-space")
            val down = submit(session, MotionEvent.ACTION_DOWN, 10); assertTrue(down.receipt.cancel(false))
            val up = submit(session, MotionEvent.ACTION_UP, 20); assertTrue(up.receipt.cancel(false))
            val legacy = CapturedStroke(UUID.randomUUID().toString(), UUID.randomUUID().toString(), 0, 9, false, listOf(ObservedPoint(-0.0, 1.0, 0, null, null, null, null)))
            val pending = backend.capture(UUID.randomUUID(), entity, "legacy-space", listOf(legacy)); assertTrue(pending.cancel(false))
            release.countDown(); authentication.get(5, TimeUnit.SECONDS)
            assertEquals(0, backend.pendingCaptures().get(5, TimeUnit.SECONDS).toInt())
            assertEquals(2, blocks(backend, entity).length())
        } finally { release.countDown(); backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }

    @Test(timeout = 20_000) fun failedClosingCheckpointDoesNotPreventOtherSessionsFromSealing() {
        val root = temporary.newFolder(); val context = platform(root); val backend = NotebookBackend(context)
        val protected = note(backend); val independent = note(backend)
        backend.authenticate("[{\"Entity\":\"$protected\"}]") { true }.get(5, TimeUnit.SECONDS)
        call(backend, JSONObject().put("op", "SetLock").put("entity", protected.toString()).put("revision", 0).put("locked", true))
        call(backend, JSONObject().put("op", "Relock"))
        val denied = backend.openCaptureSession(protected, "protected-space")
        assertThrows(ExecutionException::class.java) { submit(denied, MotionEvent.ACTION_DOWN, 10).receipt.get(5, TimeUnit.SECONDS) }
        val valid = backend.openCaptureSession(independent, "independent-space")
        submit(valid, MotionEvent.ACTION_DOWN, 10).receipt.get(5, TimeUnit.SECONDS)
        assertThrows(ExecutionException::class.java) { backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
        assertEquals(1, denied.interruptedObservations().single().points.size)
        val reopened = NotebookBackend(context)
        try { assertEquals(1, blocks(reopened, independent).length()) } finally { reopened.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }

}
