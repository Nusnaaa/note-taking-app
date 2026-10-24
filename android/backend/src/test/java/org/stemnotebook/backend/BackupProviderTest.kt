package org.stemnotebook.backend

import android.content.Context
import android.content.ContextWrapper
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.CancellationSignal
import android.os.ParcelFileDescriptor
import android.provider.DocumentsContract
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
import java.util.concurrent.CountDownLatch
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
import java.util.concurrent.atomic.AtomicInteger

/** Actual JNI/staged bytes and real host descriptors, with a synthetic provider.
 * This is not Binder/ART, a cloud provider or device durability evidence. */
@RunWith(JniBackendRunner::class)
@Config(sdk = [35])
class BackupProviderTest {
    @get:Rule val temporary = TemporaryFolder()
    private val tree = Uri.parse("content://synthetic.backups/tree/root")
    private val flags = Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
    private val password get() = "Deliberate provider backup test passphrase".toCharArray()
    private inner class Documents(val directory: File) : BackupDocuments() {
        var permitted = true
        var label = "Selected backups"
        var child = true
        var createHook: () -> Unit = {}
        var openHook: (Boolean, CancellationSignal) -> Unit = { _, _ -> }
        var corruptRead = false
        var streaming = false
        private var drained: CountDownLatch? = null
        val created = AtomicInteger()
        val opens = AtomicInteger()
        val files = mutableMapOf<Uri, File>()
        override fun inspect(tree: Uri, signal: CancellationSignal): Selection {
            signal.throwIfCanceled(); if (!permitted) throw SecurityException("private grant diagnostic")
            return Selection("root", label)
        }
        override fun create(tree: Uri, name: String): Uri {
            createHook()
            val count = created.incrementAndGet()
            assertTrue(name.startsWith("STEM-backup-") && name.endsWith(".stemenc"))
            return DocumentsContract.buildDocumentUriUsingTree(tree, "new-$count").also {
                val file = File(directory, "new-$count"); check(file.createNewFile()); files[it] = file
            }
        }
        override fun isChild(tree: Uri, document: Uri) = child
        override fun open(document: Uri, write: Boolean, signal: CancellationSignal): ParcelFileDescriptor {
            opens.incrementAndGet(); openHook(write, signal); signal.throwIfCanceled()
            if (!permitted) throw SecurityException("private provider diagnostic")
            val file = checkNotNull(files[document])
            if (streaming && write) {
                val pipe = ParcelFileDescriptor.createPipe()
                val complete = CountDownLatch(1); drained = complete
                Thread({
                    try { ParcelFileDescriptor.AutoCloseInputStream(pipe[0]).use { input -> file.outputStream().use { input.copyTo(it) } } }
                    finally { complete.countDown() }
                }, "synthetic-provider-drain").apply { isDaemon = true; start() }
                return pipe[1]
            }
            if (!write) check(drained?.await(10, TimeUnit.SECONDS) != false)
            if (!write && corruptRead) file.appendBytes(byteArrayOf(7))
            return ParcelFileDescriptor.open(file, if (write) ParcelFileDescriptor.MODE_WRITE_ONLY else ParcelFileDescriptor.MODE_READ_ONLY)
        }
    }
    private fun platform(root: File) = object : ContextWrapper(RuntimeEnvironment.getApplication()) { override fun getNoBackupFilesDir() = root }
    private fun backend(root: File, documents: BackupDocuments, fault: (String) -> Unit = {}) =
        NotebookBackend(platform(root), { context, native -> PdfWorkspace(context, native) }, backupFault = fault, backupDocuments = documents)
    private fun ready(backend: NotebookBackend): NotebookBackups.Prepared {
        val review = backend.backups().review().await(); val secret = password
        return try { backend.backups().create(review, secret, { true }, true, 8L * 1024 * 1024).await() }
        finally { secret.fill('\u0000'); review.close() }
    }
    private fun unavailable(action: () -> Unit) {
        try { action(); fail("Provider operation should fail") } catch (expected: IOException) { assertEquals("Backup operation unavailable", expected.message) }
    }
    private fun finish(backend: NotebookBackend) { backend.closeFullyAsync().get(15, TimeUnit.SECONDS) }
    @Test(timeout = 60000) fun reviewedFreshDocumentContainsOnlyVerifiedCiphertextAndNoCloudDurabilityClaim() {
        val documents = Documents(temporary.newFolder()); val backend = backend(temporary.newFolder(), documents)
        try {
            val artifact = ready(backend)
            val review = artifact.reviewProvider(tree, flags).await()
            assertEquals("Selected backups", review.label); assertEquals(0, documents.created.get())
            try { review.publish(false); fail("Unconfirmed provider write") } catch (_: IllegalArgumentException) { }
            val result = review.publish(true).await()
            assertEquals(NotebookBackups.Outcome.ProviderVerified, result.outcome)
            assertTrue(result.coversCurrentAuthoring); assertEquals(1, documents.created.get()); assertEquals(2, documents.opens.get())
            val written = checkNotNull(documents.files[result.document])
            assertArrayEquals(artifact.file.readBytes(), written.readBytes())
            assertEquals("STEMENC2", written.inputStream().use { it.readNBytes(8).toString(Charsets.US_ASCII) })
            artifact.copyRecoveryKey().also { assertEquals(64, it.size); it.fill('\u0000') }
            try { review.publish(true); fail("Replayed destination") } catch (_: IllegalStateException) { }
            artifact.close()
        } finally { finish(backend) }
    }
    @Test(timeout = 60000) fun writeOnlyPipeCanBeVerifiedWithoutInventingDescriptorDurability() {
        val documents = Documents(temporary.newFolder()).apply { streaming = true }
        // Robolectric pipe descriptors can acknowledge sync on a regular backing
        // file. Inject the unsupported-sync condition; do not call that ART evidence.
        val backend = backend(temporary.newFolder(), documents) { if (it == "provider-sync")
            throw java.io.SyncFailedException("synthetic non-seekable output") }
        try {
            val artifact = ready(backend)
            val result = artifact.reviewProvider(tree, flags).await().publish(true).await()
            assertEquals(NotebookBackups.Outcome.ProviderVerified, result.outcome)
            assertFalse(result.descriptorSyncAcknowledged)
            assertArrayEquals(artifact.file.readBytes(), documents.files[result.document]!!.readBytes())
            artifact.close()
        } finally { finish(backend) }
    }
    @Test(timeout = 60000) fun suppliedPickerBitsNeverReplaceActualAccessAndExistingDocumentsAreUntouched() {
        val documents = Documents(temporary.newFolder()); val existing = File(documents.directory, "existing").apply { writeText("keep existing content") }
        val backend = backend(temporary.newFolder(), documents)
        try {
            val artifact = ready(backend)
            try { artifact.reviewProvider(tree, Intent.FLAG_GRANT_WRITE_URI_PERMISSION); fail("Missing read grant") } catch (_: IllegalArgumentException) { }
            documents.permitted = false
            unavailable { artifact.reviewProvider(tree, flags).await() }
            assertEquals(0, documents.created.get()); assertEquals("keep existing content", existing.readText())
            documents.permitted = true
            artifact.reviewProvider(tree, flags).await().publish(true).await()
            assertEquals("keep existing content", existing.readText()); artifact.close()
        } finally { finish(backend) }
    }
    @Test(timeout = 10000) fun productionAdapterRejectsOsDeniedGrantBeforeReadingProviderMetadata() {
        val denied = object : ContextWrapper(RuntimeEnvironment.getApplication()) {
            override fun getApplicationContext(): Context = this
            override fun checkUriPermission(uri: Uri, pid: Int, uid: Int, modeFlags: Int) = PackageManager.PERMISSION_DENIED
        }
        try { BackupDocuments.platform(denied).inspect(tree, CancellationSignal()); fail("OS grant bypass") }
        catch (_: SecurityException) { }
    }
    @Test(timeout = 60000) fun changedDestinationOrRevokedGrantRequiresNewReviewWithoutCreatingAFile() {
        val documents = Documents(temporary.newFolder()); val backend = backend(temporary.newFolder(), documents)
        try {
            val artifact = ready(backend)
            val first = artifact.reviewProvider(tree, flags).await(); documents.label = "Renamed destination"
            unavailable { first.publish(true).await() }; assertEquals(0, documents.created.get())
            first.close()
            val second = artifact.reviewProvider(tree, flags).await(); documents.permitted = false
            unavailable { second.publish(true).await() }; assertEquals(0, documents.created.get()); artifact.close()
        } finally { finish(backend) }
    }
    @Test(timeout = 60000) fun corruptedPrivateArtifactIsRejectedBeforeExternalCreation() {
        val documents = Documents(temporary.newFolder()); val backend = backend(temporary.newFolder(), documents)
        try {
            val artifact = ready(backend); val review = artifact.reviewProvider(tree, flags).await()
            artifact.file.writeText("private altered bytes")
            unavailable { review.publish(true).await() }
            assertEquals(0, documents.created.get()); assertEquals(0, documents.opens.get()); artifact.close()
        } finally { finish(backend) }
    }
    @Test(timeout = 60000) fun readbackMismatchIsUncertainAndKeepsCredentialsForExplicitRecovery() {
        val documents = Documents(temporary.newFolder()).apply { corruptRead = true }
        val backend = backend(temporary.newFolder(), documents)
        try {
            val artifact = ready(backend)
            val result = artifact.reviewProvider(tree, flags).await().publish(true).await()
            assertEquals(NotebookBackups.Outcome.Uncertain, result.outcome); assertNotNull(result.document)
            assertFalse(result.coversCurrentAuthoring); assertTrue(backend.backups().needsAttention)
            artifact.copyRecoveryKey().fill('\u0000'); artifact.close()
        } finally { finish(backend) }
    }
    @Test(timeout = 60000) fun ambiguousCreationAndNonChildDocumentNeverBecomeVerifiedBackups() {
        val documents = Documents(temporary.newFolder()); val backend = backend(temporary.newFolder(), documents)
        try {
            val artifact = ready(backend)
            documents.createHook = { throw IOException("private create diagnostic") }
            val unknown = artifact.reviewProvider(tree, flags).await().publish(true).await()
            assertEquals(NotebookBackups.Outcome.Uncertain, unknown.outcome); assertNull(unknown.document)
            documents.createHook = {}; documents.child = false
            val outside = artifact.reviewProvider(tree, flags).await().publish(true).await()
            assertEquals(NotebookBackups.Outcome.Uncertain, outside.outcome); assertEquals(0, documents.opens.get()); artifact.close()
        } finally { finish(backend) }
    }
    @Test(timeout = 60000) fun revocationAfterCreationLeavesAnUncertainNewDocumentWithoutTruncatingOthers() {
        val documents = Documents(temporary.newFolder()).apply { openHook = { _, _ -> permitted = false } }
        val backend = backend(temporary.newFolder(), documents)
        try {
            val artifact = ready(backend)
            val result = artifact.reviewProvider(tree, flags).await().publish(true).await()
            assertEquals(NotebookBackups.Outcome.Uncertain, result.outcome)
            assertNotNull(result.document); assertEquals(0, documents.files[result.document]!!.length()); artifact.close()
        } finally { finish(backend) }
    }
    @Test(timeout = 60000) fun providerDeadlineCancelsCooperativeOpenAndStillReportsThePossibleNewDocument() {
        val entered = CountDownLatch(1); val released = CountDownLatch(1)
        val documents = Documents(temporary.newFolder()).apply { openHook = { _, signal ->
            signal.setOnCancelListener { released.countDown() }; entered.countDown(); check(released.await(20, TimeUnit.SECONDS))
        } }
        val backend = backend(temporary.newFolder(), documents)
        try {
            val artifact = ready(backend)
            val pending = artifact.reviewProvider(tree, flags).await().publish(true, 5000)
            assertTrue(entered.await(10, TimeUnit.SECONDS))
            assertEquals(NotebookBackups.Outcome.Uncertain, pending.await().outcome); artifact.close()
        } finally { released.countDown(); finish(backend) }
    }
    @Test(timeout = 60000) fun cancellationListenerRetirementKeepsLibraryOwnershipAndAdmissionBounded() {
        val opening = CountDownLatch(1); val openRelease = CountDownLatch(1)
        val cancelling = CountDownLatch(1); val cancelRelease = CountDownLatch(1)
        val documents = Documents(temporary.newFolder()).apply { openHook = { _, signal ->
            signal.setOnCancelListener { cancelling.countDown(); check(cancelRelease.await(30, TimeUnit.SECONDS)) }
            opening.countDown(); check(openRelease.await(30, TimeUnit.SECONDS))
        } }
        val root = temporary.newFolder(); val backend = backend(root, documents)
        val artifact = ready(backend); val pending = artifact.reviewProvider(tree, flags).await().publish(true)
        try {
            assertTrue(opening.await(15, TimeUnit.SECONDS)); assertTrue(pending.cancel()); assertTrue(cancelling.await(10, TimeUnit.SECONDS))
            openRelease.countDown()
            assertEquals(NotebookBackups.Outcome.Uncertain, pending.await().outcome)
            try { artifact.reviewProvider(tree, flags); fail("Cancellation slot released early") } catch (_: RejectedExecutionException) { }
            backend.closeAsync().get(10, TimeUnit.SECONDS)
            try { backend.closeFullyAsync().get(150, TimeUnit.MILLISECONDS); fail("Cancellation callback still owns library") } catch (_: TimeoutException) { }
            try { backend(root, documents); fail("Second owner admitted") } catch (_: IllegalStateException) { }
        } finally { openRelease.countDown(); cancelRelease.countDown(); finish(backend) }
        finish(backend(root, documents))
    }
    @Test(timeout = 60000) fun blockedProviderReadbackLeavesAuthoringAvailableAndDoesNotCoverLaterEdits() {
        val entered = CountDownLatch(1); val release = CountDownLatch(1)
        val documents = Documents(temporary.newFolder()).apply { openHook = { write, _ -> if (!write) {
            entered.countDown(); check(release.await(30, TimeUnit.SECONDS))
        } } }
        val backend = backend(temporary.newFolder(), documents)
        try {
            val artifact = ready(backend); val pending = artifact.reviewProvider(tree, flags).await().publish(true)
            try {
                assertTrue(entered.await(15, TimeUnit.SECONDS))
                val response = JSONObject(backend.request("""{"version":1,"command":{"op":"Create","kind":"Note","parent":null,"title":"new authored content"}}""").get(10, TimeUnit.SECONDS))
                assertTrue(response.getBoolean("ok"))
            } finally { release.countDown() }
            val result = pending.await(); assertEquals(NotebookBackups.Outcome.ProviderVerified, result.outcome)
            assertFalse(result.coversCurrentAuthoring); artifact.close()
        } finally { release.countDown(); finish(backend) }
    }
    @Test(timeout = 60000) fun relockRetiresDestinationReviewBeforeAnyProviderCreation() {
        val documents = Documents(temporary.newFolder()); val backend = backend(temporary.newFolder(), documents)
        try {
            val artifact = ready(backend); val selected = artifact.reviewProvider(tree, flags).await()
            backend.request("""{"version":1,"command":{"op":"Relock"}}""").get(10, TimeUnit.SECONDS)
            try { selected.publish(true); fail("Stale destination capability") } catch (_: IllegalStateException) { }
            assertEquals(0, documents.created.get())
        } finally { finish(backend) }
    }
}
