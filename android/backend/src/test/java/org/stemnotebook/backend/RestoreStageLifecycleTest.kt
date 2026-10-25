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
import java.io.IOException
import java.io.InputStream
import java.nio.file.Files
import java.nio.file.attribute.PosixFilePermissions
import java.util.UUID
import java.util.concurrent.CancellationException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
import java.util.concurrent.atomic.AtomicBoolean

/** Synthetic filesystem/provider plus actual desktop JNI, never device evidence. */
@RunWith(JniBackendRunner::class)
@Config(sdk = [35])
class RestoreStageLifecycleTest {
    @get:Rule val temporary = TemporaryFolder()
    private val password get() = "Deliberate restore cleanup test passphrase".toCharArray()
    private class Documents : RestoreDocuments() {
        var bytes = byteArrayOf()
        override fun validate(document: Uri) = Unit
        override fun open(document: Uri, signal: CancellationSignal): InputStream = ByteArrayInputStream(bytes)
    }
    private fun platform(root: File) = object : ContextWrapper(RuntimeEnvironment.getApplication()) { override fun getNoBackupFilesDir() = root }
    private fun backend(root: File, documents: Documents = Documents(), fault: (String) -> Unit = {}) =
        NotebookBackend(platform(root), { context, native -> PdfWorkspace(context, native) }, restoreDocuments = documents, restoreFault = fault)
    private fun finish(backend: NotebookBackend) { backend.closeFullyAsync().get(20, TimeUnit.SECONDS) }
    private fun call(backend: NotebookBackend, command: JSONObject): JSONObject {
        val reply = JSONObject(backend.request(JSONObject().put("version", 1).put("command", command).toString()).get(15, TimeUnit.SECONDS))
        assertTrue(reply.optString("error"), reply.getBoolean("ok")); return reply
    }
    private fun note(backend: NotebookBackend, title: String = "healthy original") = UUID.fromString(call(backend,
        JSONObject().put("op", "Create").put("kind", "Note").put("parent", JSONObject.NULL).put("title", title)).getJSONObject("data").getString("id"))
    private fun identity(root: File): String {
        val host = NativeBridge.openExisting(File(root, "notebook/library.db").absolutePath)
        return try { NativeBridge.captureLibraryIdentity(host) } finally { NativeBridge.close(host) }
    }
    private fun known(root: File, library: String): Pair<RestoreStages.Stage, File> {
        val manager = RestoreStages(File(root, "notebook/restore-staging"), File(root, "notebook/restore-retired"))
        val stage = manager.create(library)
        val file = File(stage.directory, ".stem-stream-restore-${UUID.randomUUID()}.db")
        file.writeBytes(byteArrayOf(1, 2, 3))
        Files.setPosixFilePermissions(file.toPath(), PosixFilePermissions.fromString("rw-------"))
        return stage to file
    }
    private fun archive(backend: NotebookBackend, documents: Documents) {
        val review = backend.backups().review().await(); val secret = password
        try {
            backend.backups().create(review, secret, { true }, true, 16L * 1024 * 1024).await().use { artifact ->
                documents.bytes = artifact.file.readBytes(); artifact.copyRecoveryKey().fill('\u0000')
            }
        } finally { secret.fill('\u0000'); review.close() }
    }
    private fun seed(root: File, documents: Documents = Documents(), archive: Boolean = false): String {
        val original = backend(root, documents)
        try { note(original); if (archive) archive(original, documents) } finally { finish(original) }
        return identity(root)
    }
    private fun disposed(backend: NotebookBackend, review: NotebookRestores.StageReview) {
        review.close()
        // A read-only worker barrier waits behind queued native reservation retirement.
        backend.restores().recoveredLibraries().await()
    }
    private fun classRoot(type: Class<*>): String {
        val key = type.name.replace('.', '/') + ".class"
        val resource = checkNotNull(checkNotNull(type.classLoader).getResource(key))
        return when (resource.protocol) {
            "file" -> {
                var path = java.nio.file.Paths.get(resource.toURI())
                repeat(key.split('/').size) { path = path.parent }
                path.toFile().absolutePath
            }
            "jar" -> File((resource.openConnection() as java.net.JarURLConnection).jarFileURL.toURI()).absolutePath
            else -> error("Unsupported child class resource")
        }
    }
    private fun unavailable(reason: NotebookRestores.FailureReason? = null, action: () -> Unit) {
        try { action(); fail("Invalid retirement accepted") } catch (error: IOException) {
            assertEquals("Restore operation unavailable", error.message)
            if (reason != null) assertEquals(reason, (error as NotebookRestores.Failure).reason)
        }
    }
    @Test(timeout = 90000) fun confirmedKnownDiscardUnblocksRestoreAndPreservesOriginalDrafts() {
        val root = temporary.newFolder(); val documents = Documents(); val library = seed(root, documents, true)
        val (stage, _) = known(root, library)
        val unknown = File(root, "notebook/active-capture/unknown.pending").apply { writeBytes(byteArrayOf(5, 7, 9)) }
        val backend = backend(root, documents)
        try {
            val review = backend.restores().reviewStages().await(); val item = review.items.single()
            assertTrue(item.canDiscard); assertFalse(item.containsOnlyCleanupMetadata)
            val guidance = call(backend, JSONObject().put("op", "Help").put("topic", "RestoreCleanup")).getJSONObject("data")
            assertTrue(guidance.getString("explanation").contains("cannot be undone"))
            try { review.retire(item, NotebookRestores.StageDisposition.DiscardRecognised, { true }, false); fail("Unconfirmed deletion") } catch (_: IllegalArgumentException) { }
            val result = review.retire(item, NotebookRestores.StageDisposition.DiscardRecognised, { true }, true).await()
            assertEquals(NotebookRestores.StageOutcome.Discarded, result.outcome); assertTrue(result.sourceRetired)
            assertTrue(result.removedFileBytes > 3); assertFalse(stage.directory.exists()); assertFalse(backend.restores().needsAttention)
            assertArrayEquals(byteArrayOf(5, 7, 9), unknown.readBytes())
            val secret = password
            try { backend.restores().prepare(Uri.parse("content://synthetic/backup"), Intent.FLAG_GRANT_READ_URI_PERMISSION,
                secret, false, NotebookRestores.Limits(16L * 1024 * 1024, 16L * 1024 * 1024, 0)).await().close() }
            finally { secret.fill('\u0000') }
            note(backend, "authoring after cleanup")
        } finally { finish(backend) }
        assertArrayEquals(byteArrayOf(5, 7, 9), unknown.readBytes())
    }
    @Test(timeout = 90000) fun unclassifiedBytesCanOnlyBePreservedInQuarantineAndAreDiscoverableAfterRestart() {
        val root = temporary.newFolder(); seed(root)
        val unknown = File(root, "notebook/restore-staging/unclassified").apply { writeBytes(byteArrayOf(2, 3, 5)) }
        var backend = backend(root)
        try {
            val review = backend.restores().reviewStages().await(); val item = review.items.single()
            assertFalse(item.canDiscard); assertTrue(item.canQuarantine); assertEquals(3L, item.observedFileBytes)
            try { review.retire(item, NotebookRestores.StageDisposition.DiscardRecognised, { true }, true); fail("Unknown deleted") } catch (_: IllegalArgumentException) { }
            val result = review.retire(item, NotebookRestores.StageDisposition.Quarantine, { true }, true).await()
            assertEquals(NotebookRestores.StageOutcome.Quarantined, result.outcome); assertEquals(0L, result.removedFileBytes)
            assertFalse(unknown.exists()); assertArrayEquals(byteArrayOf(2, 3, 5), File(root, "notebook/restore-retired/${result.retirement}/entry").readBytes())
            assertTrue(backend.restores().needsAttention)
        } finally { finish(backend) }
        backend = backend(root)
        try {
            val review = backend.restores().reviewStages(retired = true).await()
            assertEquals(1, review.items.size); assertFalse(review.items.single().canDiscard); assertFalse(review.items.single().canQuarantine)
            disposed(backend, review)
        } finally { finish(backend) }
    }
    @Test(timeout = 90000) fun falseOrStaleVerificationCannotDeleteChangedBytes() {
        val root = temporary.newFolder(); val library = seed(root); val (_, file) = known(root, library)
        val backend = backend(root)
        try {
            var review = backend.restores().reviewStages().await()
            unavailable(NotebookRestores.FailureReason.AccessChanged) { review.retire(review.items.single(), NotebookRestores.StageDisposition.DiscardRecognised, { false }, true).await() }
            assertTrue(file.exists())
            review = backend.restores().reviewStages().await()
            unavailable(NotebookRestores.FailureReason.StagingChanged) {
                review.retire(review.items.single(), NotebookRestores.StageDisposition.DiscardRecognised, {
                    val modified = Files.getLastModifiedTime(file.toPath()); file.writeBytes(byteArrayOf(7, 8, 9)); Files.setLastModifiedTime(file.toPath(), modified); true
                }, true).await()
            }
            assertArrayEquals(byteArrayOf(7, 8, 9), file.readBytes())
            assertTrue(File(root, "notebook/restore-retired").list()!!.isEmpty())
        } finally { finish(backend) }
    }
    @Test(timeout = 90000) fun nativeRestoreReservationAndAccessEpochExcludeInvalidOrForeignReviews() {
        val root = temporary.newFolder(); val library = seed(root); known(root, library)
        val otherRoot = temporary.newFolder(); val otherLibrary = seed(otherRoot); known(otherRoot, otherLibrary)
        val backend = backend(root); val other = backend(otherRoot)
        try {
            val review = backend.restores().reviewStages().await(); val foreign = other.restores().reviewStages().await()
            try { review.retire(foreign.items.single(), NotebookRestores.StageDisposition.Quarantine, { true }, true); fail("Foreign item accepted") } catch (_: IllegalStateException) { }
            val secret = password
            try { backend.restores().prepare(Uri.parse("content://synthetic/backup"), Intent.FLAG_GRANT_READ_URI_PERMISSION,
                secret, false, NotebookRestores.Limits(1024, 1024, 0)); fail("Held inspection lost its reservation") } catch (_: IllegalStateException) { }
            finally { secret.fill('\u0000') }
            call(backend, JSONObject().put("op", "Relock"))
            try { review.retire(review.items.single(), NotebookRestores.StageDisposition.DiscardRecognised, { true }, true); fail("Revoked review accepted") } catch (_: IllegalStateException) { }
            disposed(other, foreign)
        } finally { finish(backend); finish(other) }
    }
    @Test(timeout = 90000) fun cancellationBeforeTransferRetainsOwnershipAndLeavesSourceUntouched() {
        val root = temporary.newFolder(); val library = seed(root); val (stage, _) = known(root, library)
        val backend = backend(root); val entered = CountDownLatch(1); val release = CountDownLatch(1)
        try {
            val review = backend.restores().reviewStages().await()
            val pending = review.retire(review.items.single(), NotebookRestores.StageDisposition.DiscardRecognised, {
                entered.countDown(); check(release.await(30, TimeUnit.SECONDS)); true
            }, true)
            assertTrue(entered.await(15, TimeUnit.SECONDS)); assertTrue(pending.cancel())
            backend.closeAsync().get(15, TimeUnit.SECONDS)
            try { backend.closeFullyAsync().get(150, TimeUnit.MILLISECONDS); fail("Verification still owns work") } catch (_: TimeoutException) { }
            try { backend(root); fail("Second owner admitted") } catch (_: IllegalStateException) { }
            release.countDown(); finish(backend)
            try { pending.await(); fail("Cancelled cleanup acknowledged") } catch (_: CancellationException) { }
            assertTrue(stage.directory.exists()); assertTrue(File(root, "notebook/restore-retired").list()!!.isEmpty())
        } finally { release.countDown(); finish(backend) }
    }
    @Test(timeout = 90000) fun closeAfterTransferPreservesAnUncertainReceiptAndRestartCanFinishKnownDiscard() {
        val root = temporary.newFolder(); val library = seed(root); known(root, library)
        val entered = CountDownLatch(1); val release = CountDownLatch(1)
        var backend = backend(root) { if (it == "retirement-moved") { entered.countDown(); check(release.await(30, TimeUnit.SECONDS)) } }
        try {
            val review = backend.restores().reviewStages().await()
            val pending = review.retire(review.items.single(), NotebookRestores.StageDisposition.DiscardRecognised, { true }, true)
            assertTrue(entered.await(15, TimeUnit.SECONDS)); assertFalse(pending.cancel())
            backend.closeAsync().get(15, TimeUnit.SECONDS)
            try { backend.closeFullyAsync().get(150, TimeUnit.MILLISECONDS); fail("Transferred data still owned") } catch (_: TimeoutException) { }
            release.countDown(); finish(backend)
            val result = pending.await(); assertEquals(NotebookRestores.StageOutcome.Uncertain, result.outcome); assertTrue(result.sourceRetired)
        } finally { release.countDown(); finish(backend) }
        backend = backend(root)
        try {
            val review = backend.restores().reviewStages(retired = true).await(); assertTrue(review.items.single().canDiscard)
            assertEquals(NotebookRestores.StageOutcome.Discarded, review.retire(review.items.single(), NotebookRestores.StageDisposition.DiscardRecognised, { true }, true).await().outcome)
            assertTrue(File(root, "notebook/restore-retired").list()!!.isEmpty()); note(backend, "still healthy")
        } finally { finish(backend) }
    }
    @Test(timeout = 90000) fun inspectingAHardlinkedDatabaseNeverOpensRawPayloadDescriptorsOrDisturbsWriterLocks() {
        val root = temporary.newFolder(); val library = seed(root)
        val (stage, fake) = known(root, library); assertTrue(fake.delete())
        val database = File(root, "notebook/library.db")
        Files.createLink(File(stage.directory, fake.name).toPath(), database.toPath())
        val backend = backend(root)
        try {
            val review = backend.restores().reviewStages().await(); assertFalse(review.items.single().canDiscard)
            note(backend, "after inspection")
            assertEquals(NotebookRestores.StageOutcome.Quarantined, review.retire(review.items.single(), NotebookRestores.StageDisposition.Quarantine, { true }, true).await().outcome)
            try { NativeBridge.openExisting(database.absolutePath); fail("Writer lost SQLite ownership") } catch (_: IllegalStateException) { }
            note(backend, "after rejected duplicate")
        } finally { finish(backend) }
    }
    @Test(timeout = 90000) fun paginationCursorsAreOpaqueAndInvalidatedByAccessChanges() {
        val root = temporary.newFolder(); seed(root)
        for (name in listOf("a", "b", "c")) File(root, "notebook/restore-staging/$name").writeText(name)
        val backend = backend(root)
        try {
            val first = backend.restores().reviewStages(limit = 1).await(); val cursor = checkNotNull(first.next)
            assertEquals(1, first.items.size); disposed(backend, first)
            val second = backend.restores().reviewStages(after = cursor, limit = 1).await(); assertNotNull(second.next); disposed(backend, second)
            call(backend, JSONObject().put("op", "Relock"))
            try { backend.restores().reviewStages(after = cursor); fail("Stale cursor reused") } catch (_: IllegalStateException) { }
        } finally { finish(backend) }
    }
    @Test(timeout = 90000) fun actualAbruptNativeStageExitLeavesOnlyExplicitlyReviewedScratchForDiscard() {
        val root = temporary.newFolder(); val documents = Documents(); val library = seed(root, documents, true)
        val ciphertext = temporary.newFile().apply { writeBytes(documents.bytes) }
        val stage = LibraryOwnership.acquire(root).use {
            RestoreStages(File(root, "notebook/restore-staging"), File(root, "notebook/restore-retired")).create(library)
        }
        val paths = listOf(RestoreStageCrashProbe::class.java, LibraryOwnership::class.java, NativeBridge::class.java, kotlin.Unit::class.java)
            .map(::classRoot).distinct().joinToString(File.pathSeparator)
        val process = ProcessBuilder(File(System.getProperty("java.home"), "bin/java").absolutePath, "-Xcheck:jni",
            "-Djava.library.path=${File(checkNotNull(System.getProperty("java.library.path"))).absolutePath}", "-cp", paths,
            RestoreStageCrashProbe::class.java.name, root.absolutePath, ciphertext.absolutePath, stage.directory.absolutePath)
            .redirectErrorStream(true).start()
        val complete = process.waitFor(45, TimeUnit.SECONDS)
        if (!complete) process.destroyForcibly().waitFor(10, TimeUnit.SECONDS)
        val output = process.inputStream.bufferedReader().use { it.readText() }
        assertTrue(output, complete); assertEquals(output, 73, process.exitValue())
        assertTrue(stage.directory.list()!!.any { it.endsWith(".db") })
        val backend = backend(root)
        try {
            val review = backend.restores().reviewStages().await(); assertTrue(review.items.single().canDiscard)
            assertEquals(NotebookRestores.StageOutcome.Discarded, review.retire(review.items.single(), NotebookRestores.StageDisposition.DiscardRecognised, { true }, true).await().outcome)
            assertFalse(stage.directory.exists())
            assertTrue(call(backend, JSONObject().put("op", "List").put("parent", JSONObject.NULL)).toString().contains("healthy original"))
        } finally { finish(backend) }
    }
}
