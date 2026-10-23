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
import org.robolectric.shadows.ShadowStatFs
import java.io.File
import java.io.IOException
import java.util.UUID
import java.util.concurrent.CancellationException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger

@RunWith(JniBackendRunner::class)
@Config(sdk = [35])
class PdfRetentionTest {
    @get:Rule val temporary = TemporaryFolder()
    private val uri = Uri.parse("content://synthetic/retention")
    private fun command(backend: NotebookBackend, op: JSONObject): JSONObject {
        val response = JSONObject(backend.request(JSONObject().put("version", 1).put("command", op).toString()).get(5, TimeUnit.SECONDS))
        assertTrue(response.optString("error"), response.getBoolean("ok")); return response.optJSONObject("data") ?: JSONObject()
    }
    private fun pdf(op: String, fields: JSONObject) = JSONObject().put("op", "Pdf").put("command", fields.put("op", op))
    private fun assets(f: Fixture) = JSONObject(f.backend.request(JSONObject().put("version", 1).put("command", pdf("Assets", JSONObject().put("source", f.source.toString()))).toString()).get(5, TimeUnit.SECONDS)).getJSONArray("data")
    private fun eventually(condition: () -> Boolean) {
        val until = System.nanoTime() + TimeUnit.SECONDS.toNanos(5)
        while (!condition() && System.nanoTime() < until) Thread.sleep(5)
        assertTrue(condition())
    }
    private class Fixture(val backend: NotebookBackend, val source: UUID, val entity: UUID, val file: File, val cache: File, val opens: AtomicInteger, val grants: AtomicInteger, val permitted: AtomicBoolean) : AutoCloseable {
        override fun close() { backend.closeAsync().get(5, TimeUnit.SECONDS) }
    }
    private fun fixture(beforeOpen: (Int) -> Unit = {}): Fixture {
        val root = temporary.newFolder()
        val cache = File(root, "pdf-retention-cache").apply { mkdirs() }
        val readCache = File(root, "read-cache").apply { mkdirs() }
        ShadowStatFs.registerStats(cache, 1_000_000, 1_000_000, 1_000_000)
        ShadowStatFs.registerStats(readCache, 1_000_000, 1_000_000, 1_000_000)
        ShadowStatFs.registerStats(File(root, "notebook"), 1_000_000, 1_000_000, 1_000_000)
        val platform = object : ContextWrapper(RuntimeEnvironment.getApplication()) { override fun getNoBackupFilesDir(): File = root }
        val file = File(root, "source.pdf").apply { writeText("%PDF-synthetic acquisition"); appendBytes(ByteArray(150_000) { (it % 251).toByte() }) }
        val opens = AtomicInteger(); val grants = AtomicInteger(); val permitted = AtomicBoolean(true)
        val provider = PdfProvider({ _, _ ->
            beforeOpen(opens.incrementAndGet())
            if (!permitted.get()) throw SecurityException("synthetic permission failure")
            ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY)
        }, { grants.incrementAndGet() }, {})
        val backend = NotebookBackend(platform, pdfFactory = { _, native ->
            PdfWorkspace(readCache, PdfProvider({ _, _ -> throw IOException("Retained rendering must not open an external provider") }, {}, {}), PdfParserClient { callbacks -> FixturePdfConnection(callbacks, "ok") }, native)
        }, selectionProvider = provider)
        val entity = UUID.fromString(command(backend, JSONObject().put("op", "Create").put("kind", "Note").put("parent", JSONObject.NULL).put("title", "synthetic retention owner")).getString("id"))
        val review = backend.reviewPdfSelection(PdfSelectionManager.Target.Link(entity), uri, Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION, 10_000).await()
        val source = backend.acceptPdfSelection(review, 10_000).await().source
        return Fixture(backend, source, entity, file, cache, opens, grants, permitted)
    }
    @Test(timeout = 30_000) fun defaultLinkHasNoCopyAndConfirmedManagedAcquisitionUsesFrozenChunks() {
        fixture().use { f ->
            assertEquals(0, assets(f).length()); assertEquals(2, f.opens.get()); assertEquals(1, f.grants.get())
            val receipt = f.backend.retainPdf(f.source, PdfSelectionManager.RetentionDecision.ManagedCopy(), true, 10_000).await()
            assertEquals(f.source, receipt.source); assertEquals(1, assets(f).length()); assertEquals(4, f.opens.get())
            assertEquals(1, f.grants.get()); assertTrue(f.cache.listFiles()!!.isEmpty())
            val source = command(f.backend, pdf("Source", JSONObject().put("source", f.source.toString())))
            assertEquals("ManagedCopy", source.getString("acquisition"))
            f.permitted.set(false)
            command(f.backend, pdf("MarkMissing", JSONObject().put("source", f.source.toString())))
            val frame = f.backend.renderRetainedPdf(receipt.asset, 0, 2, 2, 10_000).await()
            assertFalse(frame.historical); assertEquals(receipt.asset, frame.retainedAsset)
            assertEquals(4, f.opens.get())
        }
    }
    @Test(timeout = 30_000) fun independentHistoricalChoiceRecordsSeparateAuthorityAndSurvivesLiveRevocation() {
        fixture().use { f ->
            val decision = PdfSelectionManager.RetentionDecision.HistoricalSnapshot("explicit independent retention", exportAllowed = false)
            val receipt = f.backend.retainPdf(f.source, decision, true, 10_000).await()
            val metadata = assets(f).getJSONObject(0)
            assertEquals("HistoricalSnapshot", metadata.getString("class"))
            assertEquals("explicit independent retention", metadata.getJSONObject("retention").getString("authority"))
            assertFalse(metadata.getJSONObject("retention").getBoolean("export_allowed"))
            command(f.backend, pdf("Revoke", JSONObject().put("source", f.source.toString())))
            val frame = f.backend.renderRetainedPdf(receipt.asset, 0, 2, 2, 10_000).await()
            assertTrue(frame.historical)
            command(f.backend, pdf("RevokeRetention", JSONObject().put("asset", receipt.asset.toString())))
            assertTrue(frame.retired)
        }
    }
    @Test(timeout = 20_000) fun unconfirmedOrInvalidRetentionChoicesDoNotReadOrMutateSource() {
        fixture().use { f ->
            assertThrows(IllegalArgumentException::class.java) { f.backend.retainPdf(f.source, PdfSelectionManager.RetentionDecision.ManagedCopy(), false) }
            assertThrows(IllegalArgumentException::class.java) { PdfSelectionManager.RetentionDecision.ManagedCopy(exportAllowed = true) }
            assertThrows(IllegalArgumentException::class.java) { PdfSelectionManager.RetentionDecision.HistoricalSnapshot(" ") }
            assertThrows(IllegalArgumentException::class.java) { PdfSelectionManager.RetentionDecision.HistoricalSnapshot("é".repeat(2049)) }
            assertEquals(2, f.opens.get()); assertEquals(0, assets(f).length())
        }
    }
    @Test(timeout = 30_000) fun changedBytesPermissionLossAndLowStorageLeaveNoRetainedAssetOrTemporaryCopy() {
        fixture().use { f ->
            f.file.appendText("changed")
            val changed = f.backend.retainPdf(f.source, PdfSelectionManager.RetentionDecision.ManagedCopy(), true, 10_000)
            assertThrows(IOException::class.java) { changed.await() }; changed.awaitRetired()
            assertEquals(0, assets(f).length()); assertTrue(f.cache.listFiles()!!.isEmpty())
            f.permitted.set(false)
            val denied = f.backend.retainPdf(f.source, PdfSelectionManager.RetentionDecision.ManagedCopy(), true, 10_000)
            assertThrows(IOException::class.java) { denied.await() }; denied.awaitRetired()
            assertEquals(0, assets(f).length()); assertTrue(f.cache.listFiles()!!.isEmpty())
            f.permitted.set(true); ShadowStatFs.registerStats(f.cache, 1, 0, 0)
            val low = f.backend.retainPdf(f.source, PdfSelectionManager.RetentionDecision.ManagedCopy(), true, 10_000)
            assertThrows(IOException::class.java) { low.await() }; low.awaitRetired()
            assertEquals(0, assets(f).length()); assertTrue(f.cache.listFiles()!!.isEmpty())
            assertEquals("ExternalLink", command(f.backend, pdf("Source", JSONObject().put("source", f.source.toString()))).getString("acquisition"))
        }
    }
    @Test(timeout = 30_000) fun lostPermissionAtFinalReopenReleasesNativeStageAndAllowsAnExplicitRetry() {
        val failure = AtomicBoolean(true)
        fixture { count -> if (count == 4 && failure.get()) throw SecurityException("synthetic final reopen failure") }.use { f ->
            val pending = f.backend.retainPdf(f.source, PdfSelectionManager.RetentionDecision.ManagedCopy(), true, 10_000)
            assertThrows(IOException::class.java) { pending.await() }; pending.awaitRetired()
            assertEquals(0, assets(f).length()); assertTrue(f.cache.listFiles()!!.isEmpty())
            failure.set(false)
            f.backend.retainPdf(f.source, PdfSelectionManager.RetentionDecision.ManagedCopy(), true, 10_000).await()
            assertEquals(1, assets(f).length())
        }
    }
    @Test(timeout = 20_000) fun blockedProviderTimeoutRetainsAdmissionWhileNotesRemainWritable() {
        val entered = CountDownLatch(1); val release = CountDownLatch(1)
        val f = fixture { count -> if (count == 3) { entered.countDown(); release.await() } }
        try {
            val pending = f.backend.retainPdf(f.source, PdfSelectionManager.RetentionDecision.ManagedCopy(), true, 1_000)
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            assertThrows(TimeoutException::class.java) { pending.await() }
            assertThrows(RejectedExecutionException::class.java) { f.backend.retainPdf(f.source, PdfSelectionManager.RetentionDecision.ManagedCopy(), true) }
            command(f.backend, JSONObject().put("op", "Append").put("entity", f.entity.toString()).put("content", JSONObject().put("type", "Text").put("value", "independent notes remain writable")))
            release.countDown(); pending.awaitRetired()
            assertTrue(f.cache.listFiles()!!.isEmpty()); assertEquals(0, assets(f).length())
        } finally { release.countDown(); f.close() }
    }
    @Test(timeout = 20_000) fun relockDuringRetentionDiscardsPendingBytesAndPreservesAcceptedLink() {
        val entered = CountDownLatch(1); val release = CountDownLatch(1)
        val f = fixture { count -> if (count == 3) { entered.countDown(); release.await() } }
        try {
            val pending = f.backend.retainPdf(f.source, PdfSelectionManager.RetentionDecision.ManagedCopy(), true, 10_000)
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            command(f.backend, JSONObject().put("op", "Relock"))
            assertThrows(CancellationException::class.java) { pending.await() }
            release.countDown(); pending.awaitRetired()
            assertEquals(0, assets(f).length()); assertTrue(f.cache.listFiles()!!.isEmpty())
            assertEquals("ExternalLink", command(f.backend, pdf("Source", JSONObject().put("source", f.source.toString()))).getString("acquisition"))
        } finally { release.countDown(); f.close() }
    }
    @Test(timeout = 30_000) fun cancellationAfterStagingBeforeFinalReopenCannotPublishAnAssetAndReleasesCapacity() {
        val entered = CountDownLatch(1); val release = CountDownLatch(1)
        val f = fixture { count -> if (count == 4) { entered.countDown(); release.await() } }
        try {
            val pending = f.backend.retainPdf(f.source, PdfSelectionManager.RetentionDecision.ManagedCopy(), true, 10_000)
            assertTrue(entered.await(5, TimeUnit.SECONDS)); pending.cancel()
            assertThrows(CancellationException::class.java) { pending.await() }
            assertEquals(0, assets(f).length())
            release.countDown(); pending.awaitRetired()
            assertTrue(f.cache.listFiles()!!.isEmpty())
            f.backend.retainPdf(f.source, PdfSelectionManager.RetentionDecision.ManagedCopy(), true, 10_000).await()
            assertEquals(1, assets(f).length())
        } finally { release.countDown(); f.close() }
    }
    @Test(timeout = 20_000) fun retentionFinalSubmissionWaitsForActualReceiptAcrossCancellationAndDeadline() {
        val root = temporary.newFolder(); ShadowStatFs.registerStats(root, 1_000_000, 1_000_000, 1_000_000)
        val file = temporary.newFile().apply { writeText("%PDF-commit retention fixture") }
        val source = UUID.randomUUID(); val asset = UUID.randomUUID(); val lease = UUID.randomUUID(); val transfer = UUID.randomUUID()
        val entered = CountDownLatch(1); val release = CountDownLatch(1); val commits = AtomicInteger()
        val digest = java.security.MessageDigest.getInstance("SHA-256").digest(file.readBytes()).joinToString("") { "%02x".format(it.toInt() and 255) }
        val provider = PdfProvider({ _, _ -> ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY) }, {}, {})
        val manager = PdfSelectionManager(provider, { _, _, _, _ -> error("No selection in this fixture") }, { _, _, _ -> error("No selection in this fixture") }, {}, {},
            retentionDirectory = root, nativePdf = { command ->
                val data = when (command.getString("op")) {
                    "PrepareRead" -> JSONObject().put("lease", lease.toString()).put("locator", uri.toString()).put("revision", digest).put("valid_for_ms", 300_000)
                    "BeginRetention" -> JSONObject().put("transfer", transfer.toString())
                    "AppendRetention" -> JSONObject().put("offset", command.getLong("offset") + command.getJSONArray("bytes").length())
                    "FinishRetention" -> { commits.incrementAndGet(); entered.countDown(); release.await(); JSONObject().put("id", asset.toString()).put("source", source.toString()) }
                    else -> JSONObject()
                }
                JSONObject().put("version", 1).put("ok", true).put("data", data)
            })
        try {
            val pending = manager.retain(source, PdfSelectionManager.RetentionDecision.ManagedCopy(), true, 1_000)
            assertTrue(entered.await(5, TimeUnit.SECONDS)); pending.cancel(); manager.invalidate()
            Thread.sleep(1_100); assertFalse(pending.isReady)
            release.countDown(); assertEquals(asset, pending.await().asset); assertEquals(1, commits.get())
            assertTrue(root.listFiles()!!.isEmpty())
        } finally { release.countDown(); manager.close() }
    }

    @Test(timeout = 20_000) fun oversizedPdfCannotEnterRetentionStagingOrLeaveAReadCache() {
        fixture().use { f ->
            java.io.RandomAccessFile(f.file, "rw").use { it.setLength(64L * 1024 * 1024 + 1) }
            val pending = f.backend.retainPdf(f.source, PdfSelectionManager.RetentionDecision.ManagedCopy(), true, 10_000)
            assertThrows(IOException::class.java) { pending.await() }; pending.awaitRetired()
            assertEquals(0, assets(f).length()); assertTrue(f.cache.listFiles()!!.isEmpty())
        }
    }
    @Test(timeout = 20_000) fun delayedNativeReadPlanCannotExtendAuthenticationLifetime() {
        val root = temporary.newFolder(); val opens = AtomicInteger(); val preparations = AtomicInteger()
        val provider = PdfProvider({ _, _ -> opens.incrementAndGet(); throw IOException("Expired plan must not open provider") }, {}, {})
        val manager = PdfSelectionManager(provider, { _, _, _, _ -> error("Unexpected selection") }, { _, _, _ -> error("Unexpected selection") }, {}, {},
            retentionDirectory = root, nativePdf = { command ->
                val data = if (command.getString("op") == "PrepareRead") {
                    preparations.incrementAndGet(); Thread.sleep(10)
                    JSONObject().put("lease", UUID.randomUUID().toString()).put("locator", uri.toString()).put("revision", "a".repeat(64)).put("valid_for_ms", 1)
                } else JSONObject()
                JSONObject().put("version", 1).put("ok", true).put("data", data)
            })
        try {
            val pending = manager.retain(UUID.randomUUID(), PdfSelectionManager.RetentionDecision.ManagedCopy(), true, 10_000)
            assertThrows(CancellationException::class.java) { pending.await() }; pending.awaitRetired()
            assertEquals(1, preparations.get()); assertEquals(0, opens.get()); assertTrue(root.listFiles()!!.isEmpty())
        } finally { manager.close() }
    }
    @Test fun startupRetiresOnlyDisposableRetentionCrashFiles() {
        val root = temporary.newFolder()
        File(root, "retention-synthetic.pending").writeText("disposable partial source")
        val unrelated = File(root, "independent-evidence.pdf").apply { writeText("untouched") }
        val provider = PdfProvider({ _, _ -> throw IOException("Unused provider") }, {}, {})
        val manager = PdfSelectionManager(provider, { _, _, _, _ -> error("Unexpected selection") }, { _, _, _ -> error("Unexpected selection") }, {}, {}, retentionDirectory = root)
        try { assertEquals(listOf(unrelated), root.listFiles()!!.toList()); assertEquals("untouched", unrelated.readText()) } finally { manager.close() }
    }

    @Test(timeout = 20_000) fun failedDisposableCacheRemovalClosesAcquisitionInsteadOfAdmittingMoreCopies() {
        var cache: File? = null
        val f = fixture { count -> if (count == 3) {
            val pending = checkNotNull(cache).listFiles()!!.single()
            check(pending.delete()); check(pending.mkdir())
            File(pending, "synthetic-blocking-entry").writeText("cleanup fault")
        } }
        cache = f.cache
        try {
            val pending = f.backend.retainPdf(f.source, PdfSelectionManager.RetentionDecision.ManagedCopy(), true, 10_000)
            assertThrows(IOException::class.java) { pending.await() }; pending.awaitRetired()
            assertThrows(IllegalStateException::class.java) { f.backend.retainPdf(f.source, PdfSelectionManager.RetentionDecision.ManagedCopy(), true) }
            assertEquals(0, assets(f).length())
            command(f.backend, JSONObject().put("op", "Append").put("entity", f.entity.toString()).put("content", JSONObject().put("type", "Text").put("value", "original authoring remains usable")))
        } finally { f.close() }
    }

}
