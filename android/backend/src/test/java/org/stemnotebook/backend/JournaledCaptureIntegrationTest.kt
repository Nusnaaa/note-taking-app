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
