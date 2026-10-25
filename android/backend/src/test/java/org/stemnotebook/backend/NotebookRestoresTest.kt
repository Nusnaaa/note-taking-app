package org.stemnotebook.backend

import android.content.ContextWrapper
import android.content.Intent
import android.net.Uri
import android.os.CancellationSignal
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import java.io.ByteArrayInputStream
import java.io.File
import java.io.InputStream
import java.io.IOException
import java.util.UUID
import java.util.concurrent.CancellationException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger

/** Actual desktop JNI in Robolectric, not device/provider durability evidence. */
@RunWith(JniBackendRunner::class)
@Config(sdk = [35])
class NotebookRestoresTest {
    @get:Rule val temporary = TemporaryFolder()
    private val uri = Uri.parse("content://synthetic/explicit-backup")
    private val flags = Intent.FLAG_GRANT_READ_URI_PERMISSION
    private val limits = NotebookRestores.Limits(16L * 1024 * 1024, 16L * 1024 * 1024, 0)
    private val password get() = "Deliberate controller restore passphrase".toCharArray()
    private fun platform(root: File) = object : ContextWrapper(RuntimeEnvironment.getApplication()) { override fun getNoBackupFilesDir() = root }
    private class Documents : RestoreDocuments() {
        var bytes = byteArrayOf()
        val allowed = AtomicBoolean(true)
        val retired = AtomicInteger()
        var opening: (CancellationSignal) -> Unit = {}
        var reading: () -> Unit = {}
        var closing: () -> Unit = {}
        override fun validate(document: Uri) { check(allowed.get()) { "private grant diagnostic" } }
        override fun open(document: Uri, signal: CancellationSignal): InputStream {
            opening(signal)
            val original = ByteArrayInputStream(bytes)
            return object : InputStream() {
                override fun read(): Int = original.read()
                override fun read(target: ByteArray, offset: Int, count: Int): Int { reading(); return original.read(target, offset, count) }
                override fun close() { retired.incrementAndGet(); try { closing() } finally { original.close() } }
            }
        }
    }
    private fun backend(root: File, documents: Documents, fault: (String) -> Unit = {}) =
        NotebookBackend(platform(root), { context, native -> PdfWorkspace(context, native) }, restoreDocuments = documents, restoreFault = fault)
    private fun call(backend: NotebookBackend, command: JSONObject): JSONObject {
        val response = JSONObject(backend.request(JSONObject().put("version", 1).put("command", command).toString()).get(15, TimeUnit.SECONDS))
        assertTrue(response.optString("error"), response.getBoolean("ok")); return response
    }
    private fun note(backend: NotebookBackend, title: String = "restored fixture") = UUID.fromString(call(backend,
        JSONObject().put("op", "Create").put("kind", "Note").put("parent", JSONObject.NULL).put("title", title)).getJSONObject("data").getString("id"))
    private fun archive(backend: NotebookBackend, documents: Documents): CharArray {
        val review = backend.backups().review().await()
        val secret = password
        return try {
            backend.backups().create(review, secret, { true }, true, limits.ciphertextBytes).await().use {
                documents.bytes = it.file.readBytes(); it.copyRecoveryKey()
            }
        } finally { secret.fill('\u0000'); review.close() }
    }
    private fun prepared(backend: NotebookBackend, secret: CharArray = password, recovery: Boolean = false, chosen: NotebookRestores.Limits = limits,
        timeout: Long = 120_000): NotebookRestores.Pending<NotebookRestores.Review> =
        try { backend.restores().prepare(uri, flags, secret, recovery, chosen, timeout) } finally { secret.fill('\u0000') }
    private fun finish(backend: NotebookBackend) { backend.closeFullyAsync().get(20, TimeUnit.SECONDS) }
    private fun unavailable(reason: NotebookRestores.FailureReason? = null, action: () -> Unit) {
        try { action(); fail("Invalid restore accepted") } catch (expected: IOException) {
            assertEquals("Restore operation unavailable", expected.message)
            if (reason != null) assertEquals(reason, (expected as NotebookRestores.Failure).reason)
        }
    }
    @Test(timeout = 90000) fun recoveredCopyPreservesInkAndDraftsAndOpensOnlyByExplicitActionAcrossRestart() {
        val root = temporary.newFolder(); val documents = Documents(); var backend = backend(root, documents)
        var recoveredId: UUID? = null
        try {
            val entity = note(backend)
            val stroke = CapturedStroke(UUID.randomUUID().toString(), UUID.randomUUID().toString(), 0, 7, false,
                listOf(ObservedPoint(-0.0, 0.125, 0, null, null, null, null)))
            assertEquals(1, backend.capture(UUID.randomUUID(), entity, "source-units", listOf(stroke)).get(15, TimeUnit.SECONDS).toInt())
            val block = call(backend, JSONObject().put("op", "Blocks").put("entity", entity).put("offset", 0).put("limit", 10)).getJSONArray("data").getJSONObject(0).getString("id")
            val source = JSONObject().put("op", "SourceInk").put("block", block)
            val before = call(backend, source).get("data").toString()
            archive(backend, documents).fill('\u0000')
            val unknown = File(root, "notebook/active-capture/unknown.pending"); unknown.writeBytes(byteArrayOf(2, 4, 6))
            val review = prepared(backend).await()
            assertEquals(1, review.summary.strokes); assertEquals(1, documents.retired.get())
            note(backend, "new live work")
            try { review.publish({ true }, false); fail("Unconfirmed publication") } catch (_: IllegalArgumentException) { }
            val result = review.publish({ true }, true).await()
            assertEquals(NotebookRestores.Outcome.Published, result.outcome)
            assertEquals("Recovered library", result.library.label); recoveredId = result.library.id
            assertEquals(before, call(backend, source).get("data").toString())
            assertArrayEquals(byteArrayOf(2, 4, 6), unknown.readBytes())
            result.library.open(platform(root)).let { recovered ->
                try {
                    assertEquals(before, call(recovered, source).get("data").toString())
                    try { result.library.open(platform(root)); fail("Recovered duplicate owner admitted") } catch (_: IllegalStateException) { }
                    note(recovered, "recovered authoring after failed duplicate")
                    val recoveredDatabase = File(root, "recovered-libraries/${result.library.id}/notebook/library.db")
                    try { NativeBridge.openExisting(recoveredDatabase.absolutePath); fail("Native duplicate admitted") } catch (_: IllegalStateException) { }
                    note(recovered, "writer locks stay owned")
                    val titles = call(recovered, JSONObject().put("op", "List").put("parent", JSONObject.NULL)).toString()
                    assertTrue(titles.contains("restored fixture")); assertFalse(titles.contains("new live work"))
                } finally { finish(recovered) }
            }
        } finally { finish(backend) }
        backend = backend(root, documents)
        try {
            val records = backend.restores().recoveredLibraries().await()
            assertEquals(1, records.libraries.size); assertEquals(recoveredId, records.libraries.single().id)
            assertEquals(NotebookRestores.Outcome.Published, records.libraries.single().publicationState)
            assertTrue(File(root, "notebook/restore-staging").list()!!.isEmpty())
        } finally { finish(backend) }
    }
    @Test(timeout = 90000) fun protectedScopesStayLockedInAnIndependentRecoveryKeyCopy() {
        val root = temporary.newFolder(); val documents = Documents(); val backend = backend(root, documents)
        try {
            val entity = note(backend)
            val scopes = """[{"Entity":"$entity"}]"""
            backend.authenticate(scopes) { true }.get(15, TimeUnit.SECONDS)
            call(backend, JSONObject().put("op", "SetLock").put("entity", entity).put("revision", 0).put("locked", true))
            val key = archive(backend, documents)
            val review = prepared(backend, key, true).await()
            assertEquals(1, review.summary.protectedScopes)
            val proofCalls = AtomicInteger()
            val copy = review.publish({ proofCalls.incrementAndGet(); true }, true).await()
            assertEquals(NotebookRestores.Outcome.Published, copy.outcome); assertTrue(proofCalls.get() > 0)
            val recovered = copy.library.open(platform(root))
            try {
                val blocked = JSONObject(recovered.request("""{"version":1,"command":{"op":"Read","id":"$entity"}}""").get(15, TimeUnit.SECONDS))
                assertFalse(blocked.getBoolean("ok"))
                recovered.authenticate(scopes) { true }.get(15, TimeUnit.SECONDS)
                assertEquals(entity.toString(), call(recovered, JSONObject().put("op", "Read").put("id", entity)).getJSONObject("data").getString("id"))
            } finally { finish(recovered) }
        } finally { finish(backend) }
    }
    @Test(timeout = 90000) fun quotaAndMalformedCredentialsRetireInputWithoutChangingTheLiveLibrary() {
        val root = temporary.newFolder(); val documents = Documents(); val backend = backend(root, documents)
        try {
            note(backend); archive(backend, documents).fill('\u0000')
            unavailable(NotebookRestores.FailureReason.QuotaExceeded) { prepared(backend, chosen = NotebookRestores.Limits(8, limits.plaintextBytes, 0)).await() }
            assertEquals(1, documents.retired.get())
            unavailable { prepared(backend, charArrayOf('\uD800')).await() }
            assertEquals(2, documents.retired.get())
            unavailable(NotebookRestores.FailureReason.QuotaExceeded) { prepared(backend, chosen = limits.copy(minimumFreeBytes = Long.MAX_VALUE)).await() }
            assertEquals(2, documents.retired.get())
            assertTrue(call(backend, JSONObject().put("op", "List").put("parent", JSONObject.NULL)).toString().contains("restored fixture"))
            assertTrue(File(root, "notebook/restore-staging").list()!!.isEmpty())
        } finally { finish(backend) }
    }
    @Test(timeout = 90000) fun failedInputRetirementAndRevokedGrantSuppressStageExposure() {
        val root = temporary.newFolder(); val documents = Documents(); val backend = backend(root, documents)
        try {
            note(backend); archive(backend, documents).fill('\u0000')
            documents.closing = { throw IOException("private descriptor detail") }
            unavailable { prepared(backend).await() }; assertEquals(1, documents.retired.get())
            documents.closing = {}; documents.allowed.set(false)
            unavailable { prepared(backend).await() }; assertEquals(1, documents.retired.get())
            documents.allowed.set(true)
            prepared(backend).await().close()
        } finally { finish(backend) }
    }
    @Test(timeout = 90000) fun cancelledBlockedReadKeepsAdmissionAndOriginalAuthoringAvailableUntilRetirement() {
        val root = temporary.newFolder(); val documents = Documents(); val backend = backend(root, documents)
        val entered = CountDownLatch(1); val release = CountDownLatch(1)
        try {
            note(backend); archive(backend, documents).fill('\u0000')
            documents.reading = { entered.countDown(); check(release.await(30, TimeUnit.SECONDS)) }
            val pending = prepared(backend)
            assertTrue(entered.await(30, TimeUnit.SECONDS)); assertTrue(pending.cancel())
            try { prepared(backend); fail("Running input lost its admission") } catch (_: RejectedExecutionException) { }
            note(backend, "authoring continues")
            release.countDown()
            try { pending.await(); fail("Cancelled restore exposed a stage") } catch (_: CancellationException) { }
            assertEquals(1, documents.retired.get()); documents.reading = {}
            prepared(backend).await().close()
        } finally { release.countDown(); finish(backend) }
    }
    @Test(timeout = 90000) fun preNativeProviderOpenAndCancellationListenerKeepFullOwnershipThroughClose() {
        val root = temporary.newFolder(); val documents = Documents(); val backend = backend(root, documents)
        val opened = CountDownLatch(1); val releaseOpen = CountDownLatch(1)
        val cancelled = CountDownLatch(1); val releaseCancel = CountDownLatch(1)
        try {
            note(backend); archive(backend, documents).fill('\u0000')
            documents.opening = { signal ->
                signal.setOnCancelListener { cancelled.countDown(); check(releaseCancel.await(30, TimeUnit.SECONDS)) }
                opened.countDown(); check(releaseOpen.await(30, TimeUnit.SECONDS))
            }
            val pending = prepared(backend)
            assertTrue(opened.await(30, TimeUnit.SECONDS))
            backend.closeAsync().get(15, TimeUnit.SECONDS)
            assertTrue(cancelled.await(15, TimeUnit.SECONDS))
            releaseOpen.countDown()
            try { backend.closeFullyAsync().get(150, TimeUnit.MILLISECONDS); fail("Cancellation listener still owns work") } catch (_: TimeoutException) { }
            try { backend(root, Documents()); fail("Second owner admitted") } catch (_: IllegalStateException) { }
            releaseCancel.countDown(); finish(backend)
            try { pending.await(); fail("Closed restore exposed a review") } catch (_: CancellationException) { }
            assertEquals(1, documents.retired.get())
        } finally { releaseOpen.countDown(); releaseCancel.countDown(); finish(backend) }
        finish(backend(root, Documents()))
    }
    @Test(timeout = 90000) fun relockInvalidatesIdleReviewAndDisposesItsPrivateStageOffWriter() {
        val root = temporary.newFolder(); val documents = Documents(); val backend = backend(root, documents)
        try {
            note(backend); archive(backend, documents).fill('\u0000')
            val review = prepared(backend).await()
            call(backend, JSONObject().put("op", "Relock"))
            try { review.publish({ true }, true); fail("Stale review published") } catch (_: IllegalStateException) { }
        } finally { finish(backend) }
        assertTrue(File(root, "notebook/restore-staging").list()!!.isEmpty())
        assertTrue(File(root, "recovered-libraries").list()!!.isEmpty())
    }
    @Test(timeout = 90000) fun ambiguousPostPublicationFailureKeepsTheRecoveredCopyAndReceipt() {
        val root = temporary.newFolder(); val documents = Documents()
        val backend = backend(root, documents) { if (it == "after-publish") throw IOException("private commit detail") }
        try {
            note(backend); archive(backend, documents).fill('\u0000')
            val copy = prepared(backend).await().publish({ true }, true).await()
            assertEquals(NotebookRestores.Outcome.Uncertain, copy.outcome); assertTrue(backend.restores().needsAttention)
            val recovered = copy.library.open(platform(root))
            try { assertTrue(call(recovered, JSONObject().put("op", "List").put("parent", JSONObject.NULL)).toString().contains("restored fixture")) }
            finally { finish(recovered) }
            assertEquals(NotebookRestores.Outcome.Uncertain, backend.restores().recoveredLibraries().await().libraries.single().publicationState)
        } finally { finish(backend) }
    }
    @Test(timeout = 90000) fun publicationEntryPreservesAnOutcomeDespiteCancellationAndBackendClose() {
        val root = temporary.newFolder(); val documents = Documents()
        val entered = CountDownLatch(1); val release = CountDownLatch(1)
        val backend = backend(root, documents) { if (it == "after-publish") { entered.countDown(); check(release.await(30, TimeUnit.SECONDS)) } }
        val pending: NotebookRestores.Pending<NotebookRestores.Publication>
        try {
            note(backend); archive(backend, documents).fill('\u0000')
            pending = prepared(backend).await().publish({ true }, true)
            assertTrue(entered.await(30, TimeUnit.SECONDS)); assertFalse(pending.cancel())
            backend.closeAsync().get(15, TimeUnit.SECONDS)
            try { backend.closeFullyAsync().get(150, TimeUnit.MILLISECONDS); fail("Post-native worker still owns work") } catch (_: TimeoutException) { }
            release.countDown(); finish(backend)
            assertEquals(NotebookRestores.Outcome.Published, pending.await().outcome)
        } finally { release.countDown(); finish(backend) }
    }
    @Test(timeout = 90000) fun idleStageCleanupRunsOffWriterAndRetainsOwnershipUntilActualRetirement() {
        val root = temporary.newFolder(); val documents = Documents()
        val entered = CountDownLatch(1); val release = CountDownLatch(1)
        val backend = backend(root, documents) { if (it == "stage-cleanup") { entered.countDown(); check(release.await(30, TimeUnit.SECONDS)) } }
        try {
            note(backend); archive(backend, documents).fill('\u0000')
            prepared(backend).await().close()
            assertTrue(entered.await(30, TimeUnit.SECONDS))
            note(backend, "writer stays available")
            backend.closeAsync().get(15, TimeUnit.SECONDS)
            try { backend.closeFullyAsync().get(150, TimeUnit.MILLISECONDS); fail("Private cleanup still owns work") } catch (_: TimeoutException) { }
            try { backend(root, Documents()); fail("Second owner admitted") } catch (_: IllegalStateException) { }
        } finally { release.countDown(); finish(backend) }
        assertTrue(File(root, "notebook/restore-staging").list()!!.isEmpty())
    }
    @Test(timeout = 90000) fun missingUncertainCopyCannotActivateAnEmptyLibrary() {
        val root = temporary.newFolder(); val documents = Documents(); val backend = backend(root, documents)
        try {
            note(backend); archive(backend, documents).fill('\u0000')
            val result = prepared(backend).await().publish({ false }, true).await()
            assertEquals(NotebookRestores.Outcome.Uncertain, result.outcome)
            try { result.library.open(platform(root)); fail("Missing copy became an empty library") } catch (_: IllegalArgumentException) { }
            assertTrue(backend.restores().needsAttention)
        } finally { finish(backend) }
    }
    @Test(timeout = 90000) fun sourceGrantRevocationBeforePublicationConsumesTheReviewWithoutPublishing() {
        val root = temporary.newFolder(); val documents = Documents(); val backend = backend(root, documents)
        try {
            note(backend); archive(backend, documents).fill('\u0000')
            val review = prepared(backend).await(); documents.allowed.set(false)
            unavailable { review.publish({ true }, true).await() }
            assertTrue(File(root, "notebook/restore-staging").list()!!.isEmpty())
            assertTrue(File(root, "recovered-libraries").list()!!.isEmpty())
        } finally { finish(backend) }
    }
    @Test(timeout = 90000) fun sourceGrantRevokedDuringFreshVerificationCannotPublishTheStagedCopy() {
        val root = temporary.newFolder(); val documents = Documents(); val backend = backend(root, documents)
        try {
            note(backend); archive(backend, documents).fill('\u0000')
            val copy = prepared(backend).await().publish({ documents.allowed.set(false); true }, true).await()
            assertEquals(NotebookRestores.Outcome.Uncertain, copy.outcome)
            try { copy.library.open(platform(root)); fail("Revoked source grant published") } catch (_: IllegalArgumentException) { }
        } finally { finish(backend) }
    }
    @Test(timeout = 90000) fun postNativePrepareStillOwnsTheLibraryBeforeReviewDelivery() {
        val root = temporary.newFolder(); val documents = Documents()
        val entered = CountDownLatch(1); val release = CountDownLatch(1)
        val backend = backend(root, documents) { if (it == "after-native") { entered.countDown(); check(release.await(30, TimeUnit.SECONDS)) } }
        try {
            note(backend); archive(backend, documents).fill('\u0000')
            val pending = prepared(backend)
            assertTrue(entered.await(30, TimeUnit.SECONDS)); assertEquals(1, documents.retired.get())
            backend.closeAsync().get(15, TimeUnit.SECONDS)
            try { backend.closeFullyAsync().get(150, TimeUnit.MILLISECONDS); fail("Post-native prepare still owns work") } catch (_: TimeoutException) { }
            try { backend(root, Documents()); fail("Second owner admitted") } catch (_: IllegalStateException) { }
            release.countDown(); finish(backend)
            try { pending.await(); fail("Closed prepare delivered a stage") } catch (_: CancellationException) { }
        } finally { release.countDown(); finish(backend) }
        assertTrue(File(root, "notebook/restore-staging").list()!!.isEmpty())
    }
    @Test(timeout = 90000) fun blockedInputRetirementKeepsNativeClosePendingUntilTheDescriptorActuallyRetires() {
        val root = temporary.newFolder(); val documents = Documents(); val backend = backend(root, documents)
        val entered = CountDownLatch(1); val release = CountDownLatch(1)
        try {
            note(backend); archive(backend, documents).fill('\u0000')
            documents.closing = { entered.countDown(); check(release.await(30, TimeUnit.SECONDS)) }
            val pending = prepared(backend)
            assertTrue(entered.await(30, TimeUnit.SECONDS))
            try { backend.closeAsync().get(150, TimeUnit.MILLISECONDS); fail("Native reader retirement still owns work") } catch (_: TimeoutException) { }
            try { backend(root, Documents()); fail("Second owner admitted") } catch (_: IllegalStateException) { }
            release.countDown(); finish(backend)
            try { pending.await(); fail("Closed input delivered a stage") } catch (_: CancellationException) { }
            assertEquals(1, documents.retired.get())
        } finally { release.countDown(); finish(backend) }
    }
    @Test(timeout = 90000) fun unknownStageBytesAndInvalidCatalogueRecordsArePreservedAndReported() {
        val root = temporary.newFolder(); val documents = Documents(); var backend = backend(root, documents)
        try { note(backend); archive(backend, documents).fill('\u0000') } finally { finish(backend) }
        val unknown = File(root, "notebook/restore-staging/unknown"); unknown.writeBytes(byteArrayOf(1, 3, 5))
        val catalogue = File(root, "recovered-libraries/${UUID.randomUUID()}"); assertTrue(catalogue.mkdir())
        File(catalogue, "recovered-library.json").writeText("bad marker")
        backend = backend(root, documents)
        try {
            assertTrue(backend.restores().needsAttention)
            unavailable(NotebookRestores.FailureReason.StagingNeedsAttention) { prepared(backend).await() }
            assertTrue(backend.restores().recoveredLibraries().await().libraries.isEmpty())
            assertArrayEquals(byteArrayOf(1, 3, 5), unknown.readBytes()); assertTrue(catalogue.isDirectory)
        } finally { finish(backend) }
    }
    @Test(timeout = 90000) fun expiredProviderOpenRequestsCancellationButRetainsItsInputUntilItActuallyReturns() {
        val root = temporary.newFolder(); val documents = Documents(); val backend = backend(root, documents)
        val entered = CountDownLatch(1); val expired = CountDownLatch(1); val release = CountDownLatch(1)
        try {
            note(backend); archive(backend, documents).fill('\u0000')
            documents.opening = { signal -> signal.setOnCancelListener { expired.countDown() }; entered.countDown(); check(release.await(30, TimeUnit.SECONDS)) }
            val pending = prepared(backend, timeout = 1000)
            assertTrue(entered.await(15, TimeUnit.SECONDS)); assertTrue(expired.await(15, TimeUnit.SECONDS))
            assertEquals(0, documents.retired.get()); assertFalse(pending.isReady)
            release.countDown()
            try { pending.await(); fail("Expired restore exposed stage") } catch (_: CancellationException) { }
            assertEquals(1, documents.retired.get())
        } finally { release.countDown(); finish(backend) }
    }
}
