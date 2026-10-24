package org.stemnotebook.backend

import android.content.ContextWrapper
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
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
import java.util.concurrent.atomic.AtomicBoolean

@RunWith(JniBackendRunner::class)
@Config(sdk = [35])
class NotebookBackupsTest {
    @get:Rule val temporary = TemporaryFolder()
    private val password get() = "Deliberate controller backup test passphrase".toCharArray()
    private fun platform(root: File) = object : ContextWrapper(RuntimeEnvironment.getApplication()) { override fun getNoBackupFilesDir() = root }
    private fun backend(root: File = temporary.newFolder(), fault: (String) -> Unit = {}) =
        NotebookBackend(platform(root), { context, native -> PdfWorkspace(context, native) }, backupFault = fault)
    private fun call(backend: NotebookBackend, command: JSONObject): JSONObject {
        val result = JSONObject(backend.request(JSONObject().put("version", 1).put("command", command).toString()).get(10, TimeUnit.SECONDS))
        assertTrue(result.optString("error"), result.getBoolean("ok")); return result
    }
    private fun note(backend: NotebookBackend) = UUID.fromString(call(backend, JSONObject().put("op", "Create").put("kind", "Note").put("parent", JSONObject.NULL).put("title", "backup fixture")).getJSONObject("data").getString("id"))
    private fun prepared(backend: NotebookBackend, maximum: Long = 16L * 1024 * 1024): NotebookBackups.Prepared {
        val review = backend.backups().review().await()
        val secret = password
        return try { backend.backups().create(review, secret, { true }, true, maximum).await() }
        finally { secret.fill('\u0000'); review.close() }
    }
    private fun unavailable(reason: NotebookBackups.FailureReason? = null, action: () -> Unit) {
        try { action(); fail("Invalid backup accepted") } catch (expected: IOException) {
            assertEquals("Backup operation unavailable", expected.message)
            if (reason != null) assertEquals(reason, (expected as NotebookBackups.Failure).reason)
        }
    }
    private fun finish(backend: NotebookBackend) { backend.closeFullyAsync().get(15, TimeUnit.SECONDS) }
    @Test(timeout = 60000) fun originalInkSurvivesEncryptedStagingAndReviewedDurablePublication() {
        val root = temporary.newFolder(); val backend = backend(root)
        try {
            val entity = note(backend)
            val stroke = CapturedStroke(UUID.randomUUID().toString(), UUID.randomUUID().toString(), 0, 7, false,
                listOf(ObservedPoint(-0.0, 0.125, 0, null, null, null, null)))
            assertEquals(1, backend.capture(UUID.randomUUID(), entity, "source-units", listOf(stroke)).get(10, TimeUnit.SECONDS).toInt())
            val block = call(backend, JSONObject().put("op", "Blocks").put("entity", entity.toString()).put("offset", 0).put("limit", 10)).getJSONArray("data").getJSONObject(0).getString("id")
            val source = JSONObject().put("op", "SourceInk").put("block", block)
            val before = call(backend, source).toString()
            val artifact = prepared(backend)
            assertTrue(artifact.includesOriginalInk)
            val target = File(temporary.newFolder(), "library.stemenc")
            try { artifact.publish(target, false); fail("Unconfirmed publication") } catch (_: IllegalArgumentException) { }
            unavailable { artifact.publish(File(root, "private.enc"), true).await() }
            val result = artifact.publish(target, true).await()
            assertEquals(NotebookBackups.Outcome.Published, result.outcome)
            assertTrue(result.coversCurrentAuthoring)
            assertEquals(artifact.bytes, target.length())
            assertEquals("STEMENC2", target.inputStream().use { it.readNBytes(8).toString(Charsets.US_ASCII) })
            artifact.copyRecoveryKey().also { assertEquals(64, it.size); it.fill('\u0000') }
            assertEquals(before, call(backend, source).toString())
            artifact.close()
        } finally { finish(backend) }
        assertTrue(File(root, "notebook/backup-staging").list()!!.isEmpty())
    }
    @Test(timeout = 60000) fun unknownPrivateDraftsBlockBackupAndRemainUntouched() {
        val root = temporary.newFolder(); val backend = backend(root)
        try {
            note(backend)
            for (folder in listOf("active-capture", "capture-recovery")) {
                val unknown = File(root, "notebook/$folder/unclassified.pending")
                unknown.writeBytes(byteArrayOf(1, 3, 5))
                unavailable(NotebookBackups.FailureReason.RecoveryRequired) { backend.backups().review().await() }
                assertArrayEquals(byteArrayOf(1, 3, 5), unknown.readBytes())
                note(backend); assertTrue(unknown.delete())
            }
            backend.backups().review().await().close()
        } finally { finish(backend) }
    }
    @Test(timeout = 60000) fun aKnownUncommittedCaptureMustBeRecoveredBeforeBackup() {
        val backend = backend()
        try {
            val entity = note(backend)
            val scopes = """[{"Entity":"$entity"}]"""
            backend.authenticate(scopes) { true }.get(10, TimeUnit.SECONDS)
            call(backend, JSONObject().put("op", "SetLock").put("entity", entity.toString()).put("revision", 0).put("locked", true))
            call(backend, JSONObject().put("op", "Relock"))
            val stroke = CapturedStroke(UUID.randomUUID().toString(), UUID.randomUUID().toString(), 0, 7, false,
                listOf(ObservedPoint(1.25, -0.0, 0, null, null, null, null)))
            assertEquals(0, backend.capture(UUID.randomUUID(), entity, "source-units", listOf(stroke)).get(10, TimeUnit.SECONDS).toInt())
            assertEquals(1, backend.pendingCaptures().get(10, TimeUnit.SECONDS).toInt())
            unavailable(NotebookBackups.FailureReason.RecoveryRequired) { backend.backups().review().await() }
            assertEquals(1, backend.pendingCaptures().get(10, TimeUnit.SECONDS).toInt())
            backend.authenticate(scopes) { true }.get(10, TimeUnit.SECONDS)
            assertEquals(0, backend.pendingCaptures().get(10, TimeUnit.SECONDS).toInt())
            prepared(backend).also { assertTrue(it.includesOriginalInk); it.close() }
        } finally { finish(backend) }
    }
    @Test(timeout = 60000) fun authoringAfterReviewInvalidatesItAndReleasesTheOldNativePlan() {
        val backend = backend()
        try {
            val review = backend.backups().review().await()
            note(backend)
            unavailable { backend.backups().create(review, password, { true }, true, 1024 * 1024).await() }
            backend.backups().review().await().close()
        } finally { finish(backend) }
    }
    @Test(timeout = 60000) fun quotaAndSinkRetirementFailureExposeNoArtifactAndLeaveAuthoringUsable() {
        val failRetire = AtomicBoolean(false)
        val root = temporary.newFolder(); val backend = backend(root) { if (it == "stage-retire" && failRetire.get()) throw IOException("private sink diagnostic") }
        try {
            unavailable(NotebookBackups.FailureReason.QuotaExceeded) { prepared(backend, 1) }
            assertTrue(File(root, "notebook/backup-staging").list()!!.isEmpty())
            failRetire.set(true); unavailable { prepared(backend) }
            assertTrue(File(root, "notebook/backup-staging").list()!!.isEmpty())
            failRetire.set(false); note(backend); prepared(backend).close()
        } finally { finish(backend) }
    }
    @Test(timeout = 60000) fun postNativeWorkerRetirementStillOwnsTheAndroidLibrary() {
        val entered = CountDownLatch(1); val release = CountDownLatch(1)
        val root = temporary.newFolder(); val backend = backend(root) {
            if (it == "after-native") { entered.countDown(); check(release.await(30, TimeUnit.SECONDS)) }
        }
        val review = backend.backups().review().await()
        val pending = backend.backups().create(review, password, { true }, true, 1024 * 1024)
        try {
            assertTrue(entered.await(30, TimeUnit.SECONDS))
            backend.closeAsync().get(10, TimeUnit.SECONDS)
            try { backend.closeFullyAsync().get(150, TimeUnit.MILLISECONDS); fail("Worker still owns resources") } catch (_: TimeoutException) { }
            try { backend(root); fail("Second owner admitted") } catch (_: IllegalStateException) { }
        } finally { release.countDown(); finish(backend) }
        try { pending.await(); fail("Cancelled job exposed key") } catch (_: java.util.concurrent.CancellationException) { }
        finish(backend(root))
    }
    @Test(timeout = 60000) fun blockedSinkRetirementAllowsAuthoringAndThenRejectsTheStaleBackup() {
        val entered = CountDownLatch(1); val release = CountDownLatch(1)
        val backend = backend { if (it == "stage-retire") { entered.countDown(); check(release.await(30, TimeUnit.SECONDS)) } }
        try {
            val review = backend.backups().review().await()
            val pending = backend.backups().create(review, password, { true }, true, 1024 * 1024)
            try { assertTrue(entered.await(30, TimeUnit.SECONDS)); note(backend) } finally { release.countDown() }
            unavailable { pending.await() }
        } finally { release.countDown(); finish(backend) }
    }
    @Test(timeout = 60000) fun existingDestinationAndCorruptedCiphertextCannotBePublished() {
        val backend = backend()
        try {
            val artifact = prepared(backend)
            val target = File(temporary.newFolder(), "backup.enc").apply { writeText("existing") }
            unavailable { artifact.publish(target, true).await() }; assertEquals("existing", target.readText())
            assertTrue(target.delete()); artifact.file.writeBytes(byteArrayOf(1))
            unavailable { artifact.publish(target, true).await() }
            assertFalse(target.exists()); assertTrue(target.parentFile!!.list()!!.isEmpty())
            artifact.copyRecoveryKey().fill('\u0000'); artifact.close()
        } finally { finish(backend) }
    }
    @Test(timeout = 60000) fun publicationAfterLinkFailureIsUncertainAndRetainsAnExplicitRecoveryPath() {
        val backend = backend { if (it == "publish-link") throw IOException("private publication diagnostic") }
        val target = File(temporary.newFolder(), "backup.enc")
        try {
            val artifact = prepared(backend)
            val result = artifact.publish(target, true).await()
            assertEquals(NotebookBackups.Outcome.Uncertain, result.outcome)
            assertFalse(result.coversCurrentAuthoring)
            assertEquals(artifact.bytes, target.length())
            artifact.copyRecoveryKey().also { assertEquals(64, it.size); it.fill('\u0000') }
            artifact.close()
        } finally { finish(backend) }
        assertTrue(target.exists())
    }
    @Test(timeout = 60000) fun aValidOlderArtifactDoesNotClaimToCoverNewAuthoring() {
        val backend = backend()
        try {
            val artifact = prepared(backend); note(backend)
            val result = artifact.publish(File(temporary.newFolder(), "older.enc"), true).await()
            assertEquals(NotebookBackups.Outcome.Published, result.outcome)
            assertFalse(result.coversCurrentAuthoring); artifact.close()
        } finally { finish(backend) }
    }
    @Test(timeout = 60000) fun relockRetiresPrivateArtifactAndItsRecoveryKeyCapability() {
        val backend = backend()
        try {
            val artifact = prepared(backend)
            call(backend, JSONObject().put("op", "Relock"))
            try { artifact.copyRecoveryKey(); fail("Relock exposed recovery key") } catch (_: IllegalStateException) { }
            try { artifact.publish(File(temporary.newFolder(), "blocked.enc"), true); fail("Relock published artifact") } catch (_: IllegalStateException) { }
        } finally { finish(backend) }
    }
    @Test(timeout = 60000) fun publicationCommitDrainsToAnOutcomeDespiteLaterCancellationAndClose() {
        val entered = CountDownLatch(1); val release = CountDownLatch(1)
        val root = temporary.newFolder(); val backend = backend(root) {
            if (it == "publish-link") { entered.countDown(); check(release.await(30, TimeUnit.SECONDS)) }
        }
        val artifact = prepared(backend)
        val target = File(temporary.newFolder(), "committing.enc")
        val pending = artifact.publish(target, true)
        try {
            assertTrue(entered.await(30, TimeUnit.SECONDS)); assertFalse(pending.cancel())
            backend.closeAsync().get(10, TimeUnit.SECONDS)
            try { backend.closeFullyAsync().get(150, TimeUnit.MILLISECONDS); fail("Publication still owns resources") } catch (_: TimeoutException) { }
            try { backend(root); fail("Second owner admitted") } catch (_: IllegalStateException) { }
        } finally { release.countDown(); finish(backend) }
        val result = pending.await()
        assertEquals(NotebookBackups.Outcome.Published, result.outcome)
        assertFalse(result.coversCurrentAuthoring)
        assertEquals(result.bytes, target.length())
        finish(backend(root))
    }
    @Test(timeout = 60000) fun cancelledPreAdmissionFileWorkRetainsOwnershipAndBoundedWorkerAdmission() {
        val entered = CountDownLatch(1); val release = CountDownLatch(1)
        val root = temporary.newFolder(); val backend = backend(root) {
            if (it == "before-native") { entered.countDown(); check(release.await(30, TimeUnit.SECONDS)) }
        }
        val review = backend.backups().review().await()
        val pending = backend.backups().create(review, password, { true }, true, 1024 * 1024)
        try {
            assertTrue(entered.await(30, TimeUnit.SECONDS)); pending.cancel()
            try { backend.backups().review(); fail("Unbounded admission") } catch (_: java.util.concurrent.RejectedExecutionException) { }
            note(backend)
            backend.closeAsync().get(10, TimeUnit.SECONDS)
            try { backend.closeFullyAsync().get(150, TimeUnit.MILLISECONDS); fail("Unadmitted file work still live") } catch (_: TimeoutException) { }
            try { backend(root); fail("Second owner admitted") } catch (_: IllegalStateException) { }
        } finally { release.countDown(); finish(backend) }
        assertTrue(File(root, "notebook/backup-staging").list()!!.isEmpty())
    }
}
