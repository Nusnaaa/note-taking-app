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
import java.util.concurrent.atomic.AtomicBoolean

@RunWith(JniBackendRunner::class)
@Config(sdk = [35])
class DeletionCaptureIntegrationTest {
    @get:Rule val temporary = TemporaryFolder()
    private fun platform(root: File) = object : ContextWrapper(RuntimeEnvironment.getApplication()) { override fun getNoBackupFilesDir(): File = root }
    private fun call(backend: NotebookBackend, command: JSONObject): JSONObject {
        val result = JSONObject(backend.request(JSONObject().put("version", 1).put("command", command).toString()).get(5, TimeUnit.SECONDS))
        assertTrue(result.optString("error"), result.getBoolean("ok")); return result
    }
    private fun create(backend: NotebookBackend, kind: String = "Note", parent: UUID? = null): UUID = UUID.fromString(call(backend, JSONObject().put("op", "Create").put("kind", kind).put("parent", parent?.toString() ?: JSONObject.NULL).put("title", "deletion fixture")).getJSONObject("data").getString("id"))
    private fun source() = CapturedStroke(UUID.randomUUID().toString(), UUID.randomUUID().toString(), 0, 7, false, listOf(ObservedPoint(-0.0, 1.25, 0, null, null, null, null)), StrokeEnd.Interrupted(StrokeInterruption.Recovery))
    private fun identity(root: File): UUID {
        val native = NativeBridge.open(File(root, "notebook/library.db").absolutePath)
        return try { UUID.fromString(NativeBridge.captureLibraryIdentity(native)) } finally { NativeBridge.close(native) }
    }
    private fun event(session: JournaledCaptureSession, action: Int, time: Long): JournaledCaptureSession.Submission {
        val property = MotionEvent.PointerProperties().apply { id = 7; toolType = MotionEvent.TOOL_TYPE_STYLUS }
        val coordinate = MotionEvent.PointerCoords().apply { x = 0.125f; y = 3.5f; pressure = 0.5f }
        val input = MotionEvent.obtain(10, time, action, 1, arrayOf(property), arrayOf(coordinate), 0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_STYLUS, 0)
        return try { session.accept(input).single().also { it.receipt.get(5, TimeUnit.SECONDS) } } finally { input.recycle() }
    }
    @Test(timeout = 30_000) fun reviewedSubtreeDiscardsOnlySealedCopiesAndRetainsActiveUnrelatedAndUnknownRecovery() {
        val root = temporary.newFolder(); val context = platform(root); val seed = NotebookBackend(context)
        val folder: UUID; val child: UUID; val unrelated: UUID
        try { folder = create(seed, "Notebook"); child = create(seed, parent = folder); unrelated = create(seed) }
        finally { seed.closeFullyAsync().get(5, TimeUnit.SECONDS) }
        val journal = ActiveCaptureJournal(File(root, "notebook/active-capture"), identity(root))
        val sealed = source(); val outside = source()
        journal.checkpoint(child, "fixed-space", 0, sealed); journal.checkpoint(unrelated, "fixed-space", 0, outside)
        val unknownFile = File(root, "notebook/active-capture/active-synthetic.incomplete").apply { writeText("incomplete synthetic evidence") }
        val backend = NotebookBackend(context)
        try {
            val session = backend.openCaptureSession(child, "fixed-space"); val active = event(session, MotionEvent.ACTION_DOWN, 10)
            // Simulate an admitted legacy record awaiting its acknowledgement while the backend writer is idle.
            val legacy = CaptureOutbox(File(root, "notebook/capture-recovery")); val legacyTicket = UUID.randomUUID()
            legacy.enqueue(legacyTicket, CaptureRequest.encode(legacyTicket, child, "fixed-space", listOf(source().copy(end = StrokeEnd.Completed))))
            legacy.enqueue(UUID.randomUUID(), "unclassified synthetic record")
            val review = backend.prepareDeletionWithCaptures(folder, false).get(5, TimeUnit.SECONDS)
            assertEquals(3, review.drafts.size); assertEquals(2, review.drafts.count { it.sealed }); assertEquals(2, review.unclassifiedRecovery)
            assertTrue(review.drafts.all { it.entity == child }); assertFalse(review.drafts.any { it.ticket == UUID.fromString(outside.id) })
            assertThrows(IllegalArgumentException::class.java) { backend.confirmDeletionWithCaptures(review, CaptureDeletionDisposition.DiscardReviewedSealed, false) }
            val receipt = backend.confirmDeletionWithCaptures(review, CaptureDeletionDisposition.DiscardReviewedSealed, true).get(5, TimeUnit.SECONDS)
            assertEquals(2, receipt.draftsRemoved); assertEquals(1, receipt.keptActive); assertEquals(0, receipt.cleanupUnconfirmed)
            assertFalse(journal.contains(UUID.fromString(sealed.id))); assertTrue(journal.contains(UUID.fromString(outside.id)))
            assertEquals(active.stroke.points, journal.read(UUID.fromString(active.stroke.id)).stroke.points)
            assertEquals(1, legacy.pendingCount()); assertTrue(unknownFile.exists())
            assertEquals(folder.toString(), call(backend, JSONObject().put("op", "TrashEntries").put("offset", 0).put("limit", 10)).getJSONArray("data").getJSONObject(0).getString("id"))
        } finally { backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
    @Test(timeout = 30_000) fun keepAndRelockRechecksPreserveExactPrivateDraftBytes() {
        val root = temporary.newFolder(); val context = platform(root); val seed = NotebookBackend(context); val entity: UUID
        try { entity = create(seed) } finally { seed.closeFullyAsync().get(5, TimeUnit.SECONDS) }
        val journal = ActiveCaptureJournal(File(root, "notebook/active-capture"), identity(root)); val original = source(); val ticket = UUID.fromString(original.id)
        journal.checkpoint(entity, "fixed-space", 0, original); val before = journal.checksum(ticket)
        val backend = NotebookBackend(context)
        try {
            val stale = backend.prepareDeletionWithCaptures(entity, false).get(5, TimeUnit.SECONDS)
            call(backend, JSONObject().put("op", "Relock"))
            assertThrows(ExecutionException::class.java) { backend.confirmDeletionWithCaptures(stale, CaptureDeletionDisposition.DiscardReviewedSealed, true).get(5, TimeUnit.SECONDS) }
            assertEquals(before, journal.checksum(ticket)); call(backend, JSONObject().put("op", "Read").put("id", entity.toString()))
            val review = backend.prepareDeletionWithCaptures(entity, false).get(5, TimeUnit.SECONDS)
            val receipt = backend.confirmDeletionWithCaptures(review, CaptureDeletionDisposition.KeepDrafts, true).get(5, TimeUnit.SECONDS)
            assertEquals(1, receipt.keptSealed); assertEquals(0, receipt.draftsRemoved); assertEquals(before, journal.checksum(ticket))
            assertThrows(ExecutionException::class.java) { backend.reviewInterruptedCapture(ticket).get(5, TimeUnit.SECONDS) }
            backend.authenticate("[{\"Entity\":\"$entity\"}]") { true }.get(5, TimeUnit.SECONDS)
            assertEquals(original.points, backend.reviewInterruptedCapture(ticket).get(5, TimeUnit.SECONDS).stroke.points)
            val permanent = backend.prepareDeletionWithCaptures(entity, true).get(5, TimeUnit.SECONDS)
            assertEquals(1, backend.confirmDeletionWithCaptures(permanent, CaptureDeletionDisposition.DiscardReviewedSealed, true).get(5, TimeUnit.SECONDS).draftsRemoved)
            assertFalse(journal.contains(ticket))
        } finally { backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
    @Test(timeout = 30_000) fun changedActivePrefixInvalidatesDraftReviewWithoutTrashingOrDiscarding() {
        val backend = NotebookBackend(platform(temporary.newFolder()))
        try {
            val entity = create(backend); val session = backend.openCaptureSession(entity, "fixed-space")
            event(session, MotionEvent.ACTION_DOWN, 10)
            val review = backend.prepareDeletionWithCaptures(entity, false).get(5, TimeUnit.SECONDS)
            val moved = event(session, MotionEvent.ACTION_MOVE, 20)
            assertThrows(ExecutionException::class.java) { backend.confirmDeletionWithCaptures(review, CaptureDeletionDisposition.DiscardReviewedSealed, true).get(5, TimeUnit.SECONDS) }
            call(backend, JSONObject().put("op", "Read").put("id", entity.toString()))
            assertEquals(1, backend.pendingCaptures().get(5, TimeUnit.SECONDS).toInt())
            assertTrue(session.interrupt().single().receipt.get(5, TimeUnit.SECONDS).committed)
            val blocks = call(backend, JSONObject().put("op", "Blocks").put("entity", entity.toString()).put("offset", 0).put("limit", 10)).getJSONArray("data")
            val ink = call(backend, JSONObject().put("op", "SourceInk").put("block", blocks.getJSONObject(0).getString("id"))).getJSONArray("data").getJSONObject(0)
            assertEquals(moved.stroke.points.size, ink.getJSONArray("points").length())
            val fresh = backend.prepareDeletionWithCaptures(entity, false).get(5, TimeUnit.SECONDS)
            assertTrue(fresh.drafts.isEmpty())
            assertEquals(0, backend.confirmDeletionWithCaptures(fresh, CaptureDeletionDisposition.DiscardReviewedSealed, true).get(5, TimeUnit.SECONDS).draftsRemoved)
        } finally { backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
    @Test(timeout = 30_000) fun cleanupFailureReportsCommittedDeletionWithoutFalselyAcknowledgingDraftRemoval() {
        val root = temporary.newFolder(); val context = platform(root); val seed = NotebookBackend(context); val entity: UUID
        try { entity = create(seed) } finally { seed.closeFullyAsync().get(5, TimeUnit.SECONDS) }
        val journal = ActiveCaptureJournal(File(root, "notebook/active-capture"), identity(root)); val original = source(); val ticket = UUID.fromString(original.id)
        journal.checkpoint(entity, "fixed-space", 0, original); val before = journal.checksum(ticket); val armed = AtomicBoolean(false)
        val backend = NotebookBackend(context, { platform, native -> PdfWorkspace(platform, native) }, captureJournalFault = { phase -> if (phase == "before-delete" && armed.getAndSet(false)) throw java.io.IOException("synthetic cleanup failure") })
        try {
            val review = backend.prepareDeletionWithCaptures(entity, false).get(5, TimeUnit.SECONDS); armed.set(true)
            val receipt = backend.confirmDeletionWithCaptures(review, CaptureDeletionDisposition.DiscardReviewedSealed, true).get(5, TimeUnit.SECONDS)
            assertEquals(0, receipt.draftsRemoved); assertEquals(1, receipt.cleanupUnconfirmed); assertTrue(backend.writerStatus().needsAttention)
            assertEquals(before, journal.checksum(ticket)); assertEquals(1, call(backend, JSONObject().put("op", "TrashEntries").put("offset", 0).put("limit", 10)).getJSONArray("data").length())
            backend.authenticate("[{\"Entity\":\"$entity\"}]") { true }.get(5, TimeUnit.SECONDS)
            val recovery = backend.reviewInterruptedCapture(ticket).get(5, TimeUnit.SECONDS)
            assertEquals(original.points, recovery.stroke.points); backend.discardInterruptedCapture(recovery, true).get(5, TimeUnit.SECONDS)
        } finally { backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
}
