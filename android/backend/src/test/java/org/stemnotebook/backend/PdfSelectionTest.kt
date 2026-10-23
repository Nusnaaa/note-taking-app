package org.stemnotebook.backend

import android.content.ContextWrapper
import android.content.Intent
import android.net.Uri
import android.os.ParcelFileDescriptor
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import java.io.File
import java.io.IOException
import java.security.MessageDigest
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger

@RunWith(JniBackendRunner::class)
@Config(sdk = [35])
class PdfSelectionTest {
    @get:Rule val temporary = TemporaryFolder()
    private val uri = Uri.parse("content://synthetic/picker")
    private val flags = Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
    private fun hash(file: File) = MessageDigest.getInstance("SHA-256").digest(file.readBytes()).joinToString("") { "%02x".format(it.toInt() and 255) }
    private fun call(backend: NotebookBackend, command: JSONObject): JSONObject = JSONObject(backend.request(JSONObject().put("version", 1).put("command", command).toString()).get(5, TimeUnit.SECONDS))
    private fun command(backend: NotebookBackend, command: JSONObject): JSONObject {
        val response = call(backend, command); assertTrue(response.optString("error"), response.getBoolean("ok")); return response
    }
    private fun pdf(op: String, source: UUID) = JSONObject().put("op", "Pdf").put("command", JSONObject().put("op", op).put("source", source.toString()))
    private class Fixture(val backend: NotebookBackend, val file: File, val context: UUID, val opened: AtomicInteger, val retained: AtomicInteger, val permitted: AtomicBoolean) : AutoCloseable {
        override fun close() { backend.closeAsync().get(5, TimeUnit.SECONDS) }
    }
    private fun fixture(beforeOpen: () -> Unit = {}): Fixture {
        val root = temporary.newFolder()
        val context = object : ContextWrapper(RuntimeEnvironment.getApplication()) { override fun getNoBackupFilesDir(): File = root }
        val file = File(root, "selected.pdf").apply { writeText("%PDF-synthetic selected source") }
        val opened = AtomicInteger(); val retained = AtomicInteger(); val permitted = AtomicBoolean(true)
        val provider = PdfProvider({ _, _ ->
            opened.incrementAndGet(); beforeOpen()
            if (!permitted.get()) throw SecurityException("synthetic permission loss")
            ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY)
        }, { retained.incrementAndGet() }, {})
        val backend = NotebookBackend(context, pdfFactory = { platform, native -> PdfWorkspace(platform, native) }, selectionProvider = provider)
        val created = command(backend, JSONObject().put("op", "Create").put("kind", "Note").put("parent", JSONObject.NULL).put("title", "synthetic source owner"))
        return Fixture(backend, file, UUID.fromString(created.getJSONObject("data").getString("id")), opened, retained, permitted)
    }
    private fun review(f: Fixture, source: UUID? = null, timeout: Long = 10_000) =
        f.backend.reviewPdfSelection(if (source == null) PdfSelectionManager.Target.Link(f.context) else PdfSelectionManager.Target.Relink(source), uri, flags, timeout).await()
    private fun append(f: Fixture) = command(f.backend, JSONObject().put("op", "Append").put("entity", f.context.toString()).put("content", JSONObject().put("type", "Text").put("value", "independent authored observation")))
    @Test(timeout = 20_000) fun selectionIsNonMutatingUntilAcceptedAndPersistsOnlyReadGrantOnCommit() {
        fixture().use { f ->
            val prepared = review(f)
            assertFalse(prepared.changedRevision)
            assertEquals(0, f.retained.get()); assertEquals(1, f.opened.get())
            assertFalse(call(f.backend, pdf("Source", prepared.source)).getBoolean("ok"))
            val receipt = f.backend.acceptPdfSelection(prepared, 10_000).await()
            assertEquals(prepared.source, receipt.source)
            assertEquals(1, f.retained.get()); assertEquals(2, f.opened.get())
            val source = command(f.backend, pdf("Source", receipt.source)).getJSONObject("data")
            assertEquals(hash(f.file), source.getString("revision"))
            assertEquals("ExternalLink", source.getString("acquisition"))
            assertEquals(uri.toString(), source.getString("locator"))
            assertThrows(IllegalStateException::class.java) { f.backend.acceptPdfSelection(prepared) }
        }
    }
    @Test(timeout = 20_000) fun sameRevisionAndChangedEditionCanBothRestoreARevokedStableSource() {
        fixture().use { f ->
            val source = f.backend.acceptPdfSelection(review(f), 10_000).await().source
            command(f.backend, pdf("Revoke", source))
            val same = review(f, source)
            assertFalse(same.changedRevision)
            assertEquals("Revoked", command(f.backend, pdf("Source", source)).getJSONObject("data").getString("availability"))
            assertEquals(source, f.backend.acceptPdfSelection(same, 10_000).await().source)
            val before = hash(f.file)
            command(f.backend, pdf("Revoke", source))
            f.file.writeText("%PDF-different selected edition")
            val changed = review(f, source)
            assertTrue(changed.changedRevision)
            assertEquals(source, f.backend.acceptPdfSelection(changed, 10_000).await().source)
            val restored = command(f.backend, pdf("Source", source)).getJSONObject("data")
            assertEquals("Available", restored.getString("availability"))
            assertEquals(hash(f.file), restored.getString("revision"))
            assertEquals(before, restored.getJSONArray("previous_revisions").getString(0))
        }
    }
    @Test(timeout = 20_000) fun rejectionAndRelockRetireReviewWithoutGrantingOrCreatingSource() {
        fixture().use { f ->
            val rejected = review(f); rejected.close()
            assertFalse(call(f.backend, pdf("Source", rejected.source)).getBoolean("ok"))
            assertThrows(IllegalStateException::class.java) { f.backend.acceptPdfSelection(rejected) }
            val retired = review(f)
            command(f.backend, JSONObject().put("op", "Relock"))
            assertThrows(IllegalStateException::class.java) { retired.changedRevision }
            assertThrows(IllegalStateException::class.java) { f.backend.acceptPdfSelection(retired) }
            assertEquals(0, f.retained.get())
        }
    }
    @Test(timeout = 20_000) fun bytesOrPermissionChangingAfterReviewCannotApplyTheOldSelection() {
        fixture().use { f ->
            val changed = review(f)
            f.file.writeText("%PDF-source changed after review")
            val changing = f.backend.acceptPdfSelection(changed, 10_000)
            assertThrows(IOException::class.java) { changing.await() }
            changing.awaitRetired()
            assertFalse(call(f.backend, pdf("Source", changed.source)).getBoolean("ok"))
            val denied = review(f); f.permitted.set(false)
            val lostPermission = f.backend.acceptPdfSelection(denied, 10_000)
            assertThrows(IOException::class.java) { lostPermission.await() }
            lostPermission.awaitRetired()
            assertFalse(call(f.backend, pdf("Source", denied.source)).getBoolean("ok"))
            append(f)
        }
    }
    @Test(timeout = 20_000) fun staleNativeGenerationAndLockedContextFailBeforeAnyUnreviewedChange() {
        fixture().use { f ->
            val stale = review(f); append(f)
            val outdated = f.backend.acceptPdfSelection(stale, 10_000)
            assertThrows(IOException::class.java) { outdated.await() }
            outdated.awaitRetired()
            assertFalse(call(f.backend, pdf("Source", stale.source)).getBoolean("ok"))
            f.backend.authenticate("[{\"Entity\":\"${f.context}\"}]", NativeBridge.Verification { it.isNotEmpty() }).get(5, TimeUnit.SECONDS)
            val revision = command(f.backend, JSONObject().put("op", "Read").put("id", f.context.toString())).getJSONObject("data").getLong("revision")
            command(f.backend, JSONObject().put("op", "SetLock").put("entity", f.context.toString()).put("revision", revision).put("locked", true))
            command(f.backend, JSONObject().put("op", "Relock"))
            val opened = f.opened.get()
            assertThrows(IOException::class.java) { review(f) }
            assertEquals(opened, f.opened.get())
        }
    }
    @Test(timeout = 20_000) fun invalidPickerDataAndNonPdfSignatureDoNotCreateSourceMetadata() {
        fixture().use { f ->
            assertThrows(IllegalArgumentException::class.java) { f.backend.reviewPdfSelection(PdfSelectionManager.Target.Link(f.context), uri, 0) }
            assertThrows(IllegalArgumentException::class.java) { f.backend.reviewPdfSelection(PdfSelectionManager.Target.Link(f.context), Uri.parse("https://example.invalid/source"), flags) }
            f.file.writeText("not a PDF")
            assertThrows(IOException::class.java) { review(f) }
            assertEquals(0, f.retained.get())
        }
    }
    @Test(timeout = 20_000) fun timeoutRetainsAdmissionForUncooperativeProviderWhileNotesStayWritable() {
        val entered = CountDownLatch(1); val release = CountDownLatch(1)
        val f = fixture { entered.countDown(); release.await() }
        try {
            val pending = f.backend.reviewPdfSelection(PdfSelectionManager.Target.Link(f.context), uri, flags, 1_000)
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            assertThrows(TimeoutException::class.java) { pending.await() }
            assertThrows(RejectedExecutionException::class.java) { f.backend.reviewPdfSelection(PdfSelectionManager.Target.Link(f.context), uri, flags) }
            append(f)
        } finally { release.countDown(); f.close() }
    }
    @Test(timeout = 20_000) fun cancellationAfterNativeCommitAcceptanceWaitsForTheActualReceipt() {
        val file = temporary.newFile().apply { writeText("%PDF-commit boundary fixture") }
        val target = PdfSelectionManager.Target.Link(UUID.randomUUID())
        val reviewId = UUID.randomUUID(); val sourceId = UUID.randomUUID()
        val committing = CountDownLatch(1); val release = CountDownLatch(1)
        val provider = PdfProvider({ _, _ -> ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY) }, {}, {})
        val manager = PdfSelectionManager(provider,
            { requested, locator, digest, proof ->
                assertTrue(proof.verify(UUID.randomUUID().toString(), requested, locator, digest))
                JSONObject().put("review", reviewId.toString()).put("source", sourceId.toString()).put("changed_revision", false).put("affected_anchors", 0)
            },
            { review, confirmed, proof ->
                assertEquals(reviewId.toString(), review); assertTrue(confirmed)
                assertTrue(proof.verify(UUID.randomUUID().toString(), target.json().toString(), uri.toString(), hash(file)))
                committing.countDown(); release.await()
                JSONObject().put("id", sourceId.toString())
            }, {}, {},
        )
        try {
            val prepared = manager.review(target, uri, Intent.FLAG_GRANT_READ_URI_PERMISSION, 10_000).await()
            val pending = manager.accept(prepared, 1_000)
            assertTrue(committing.await(5, TimeUnit.SECONDS))
            pending.cancel(); manager.invalidate()
            Thread.sleep(1_100)
            assertFalse(pending.isReady)
            release.countDown()
            assertEquals(sourceId, pending.await().source)
        } finally { release.countDown(); manager.close() }
    }

    @Test(timeout = 15_000) fun providerFailurePublishesWhileOwnDeadlineAndProviderRetirementAreBlocked() {
        val entered = CountDownLatch(1); val releaseProvider = CountDownLatch(1)
        val blockedTimer = CountDownLatch(1); val releaseTimer = CountDownLatch(1)
        val file = temporary.newFile().apply { writeText("%PDF-resource boundary fixture") }
        val provider = PdfProvider({ _, _ -> entered.countDown(); releaseProvider.await(); ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY) }, {}, {})
        val manager = PdfSelectionManager(provider,
            { _, _, _, _ -> error("Timed-out provider must not prepare a native plan") },
            { _, _, _ -> error("Unexpected acceptance") }, {}, {},
        )
        val scheduler = org.robolectric.util.ReflectionHelpers.getField<java.util.concurrent.ScheduledThreadPoolExecutor>(manager, "deadlines")
        scheduler.execute { blockedTimer.countDown(); releaseTimer.await() }
        try {
            assertTrue(blockedTimer.await(5, TimeUnit.SECONDS))
            val pending = manager.review(PdfSelectionManager.Target.Link(UUID.randomUUID()), uri, Intent.FLAG_GRANT_READ_URI_PERMISSION, 1_000)
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            assertThrows(IOException::class.java) { pending.await() }
            assertThrows(RejectedExecutionException::class.java) { manager.review(PdfSelectionManager.Target.Link(UUID.randomUUID()), uri, Intent.FLAG_GRANT_READ_URI_PERMISSION) }
        } finally { releaseTimer.countDown(); releaseProvider.countDown(); manager.close() }
    }

}
