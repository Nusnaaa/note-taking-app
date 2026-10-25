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
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

@RunWith(JniBackendRunner::class)
@Config(sdk = [35])
class BackupCatalogueIntegrationTest {
    @get:Rule val temporary = TemporaryFolder()
    private fun backend(root: File, fault: (String) -> Unit = {}) = NotebookBackend(
        object : ContextWrapper(RuntimeEnvironment.getApplication()) { override fun getNoBackupFilesDir() = root },
        { context, native -> PdfWorkspace(context, native) }, backupFault = fault)
    private fun call(backend: NotebookBackend, command: JSONObject): JSONObject {
        val response = JSONObject(backend.request(JSONObject().put("version", 1).put("command", command).toString()).get(15, TimeUnit.SECONDS))
        assertTrue(response.toString(), response.getBoolean("ok")); return response
    }
    private fun note(backend: NotebookBackend) = call(backend, JSONObject().put("op", "Create").put("kind", "Note").put("parent", JSONObject.NULL).put("title", "private catalogue source title"))
    private fun prepared(backend: NotebookBackend): NotebookBackups.Prepared {
        val review = backend.backups().review().await()
        val password = "Deliberate catalogue integration password".toCharArray()
        return try { backend.backups().create(review, password, { true }, true, 16L * 1024 * 1024).await() }
        finally { password.fill('\u0000'); review.close() }
    }
    private fun finish(backend: NotebookBackend) { backend.closeFullyAsync().get(15, TimeUnit.SECONDS) }
    @Test(timeout = 60000) fun actualPublicationIsRecordedWithoutChangingSourceAndSurvivesBackendRestart() {
        val root = temporary.newFolder(); var backend = backend(root)
        try {
            note(backend)
            assertFalse(backend.backups().status().await().currentAuthoringWasPublished)
            val artifact = prepared(backend)
            val target = File(temporary.newFolder(), "selected.stemenc")
            val receipt = artifact.publish(target, true).await()
            assertEquals(NotebookBackups.Outcome.Published, receipt.outcome)
            assertTrue(receipt.catalogueRecorded)
            artifact.close()
            val record = backend.backups().status().await()
            assertTrue(record.currentAuthoringWasPublished)
            assertEquals(target.absolutePath, record.lastPublication!!.destination)
            assertEquals(target.length(), record.lastPublication!!.bytes)
            val old = record.lastPublication
            finish(backend); backend = backend(root)
            assertEquals(old, backend.backups().status().await().lastPublication)
            assertTrue(backend.backups().status().await().currentAuthoringWasPublished)
            note(backend)
            assertFalse(backend.backups().status().await().currentAuthoringWasPublished)
            assertEquals(old, backend.backups().history().await().single())
        } finally { finish(backend) }
    }
    @Test(timeout = 60000) fun pendingCaptureEvidenceAndAppLockCannotProduceAFalseCurrentBackupStatus() {
        val root = temporary.newFolder(); val backend = backend(root)
        try {
            note(backend)
            val artifact = prepared(backend)
            assertTrue(artifact.publish(File(temporary.newFolder(), "selected.stemenc"), true).await().catalogueRecorded)
            artifact.close()
            val pending = File(root, "notebook/active-capture/unclassified")
            pending.writeText("preserved pending evidence")
            val state = backend.backups().status().await()
            assertFalse(state.authoringQuiescent); assertFalse(state.currentAuthoringWasPublished)
            assertNotNull(state.currentCommittedSnapshotPublication)
            assertEquals("preserved pending evidence", pending.readText())
            backend.authenticate("[\"App\"]") { true }.get(15, TimeUnit.SECONDS)
            call(backend, JSONObject().put("op", "SetAppLock").put("enabled", true))
            call(backend, JSONObject().put("op", "Relock"))
            try { backend.backups().status().await(); fail("Locked history exposed") } catch (_: IOException) { }
            try { backend.backups().history().await(); fail("Locked records exposed") } catch (_: IOException) { }
        } finally { finish(backend) }
    }
    @Test(timeout = 60000) fun unresolvedAuthoringFailureCannotClaimCompleteCurrentCoverage() {
        val root = temporary.newFolder(); val backend = backend(root)
        try {
            note(backend); val artifact = prepared(backend)
            assertTrue(artifact.publish(File(temporary.newFolder(), "selected.stemenc"), true).await().catalogueRecorded)
            artifact.close()
            val command = JSONObject().put("op", "Rename").put("entity", java.util.UUID.randomUUID().toString()).put("revision", 0).put("title", "unpersisted attempt")
            val response = JSONObject(backend.request(JSONObject().put("version", 1).put("command", command).toString()).get(15, TimeUnit.SECONDS))
            assertFalse(response.getBoolean("ok")); assertTrue(backend.writerStatus().needsAttention)
            val status = backend.backups().status().await()
            assertNotNull(status.currentCommittedSnapshotPublication)
            assertFalse(status.authoringQuiescent); assertFalse(status.currentAuthoringWasPublished)
        } finally { finish(backend) }
    }
    @Test(timeout = 60000) fun publicationAndCatalogueFailureAreAcknowledgedSeparately() {
        val root = temporary.newFolder(); val backend = backend(root) { if (it == "catalogue-before-commit") throw IOException("injected catalogue failure") }
        try {
            note(backend)
            val artifact = prepared(backend); val target = File(temporary.newFolder(), "selected.stemenc")
            val result = artifact.publish(target, true).await()
            assertEquals(NotebookBackups.Outcome.Published, result.outcome)
            assertTrue(result.coversCurrentAuthoring); assertFalse(result.catalogueRecorded)
            assertTrue(target.isFile); artifact.close()
            assertTrue(backend.backups().needsAttention)
            assertNull(backend.backups().status().await().lastPublication)
        } finally { finish(backend) }
    }
    @Test(timeout = 60000) fun fullCloseOwnsAnEnteredCatalogueCommitUntilItsActualRetirement() {
        val root = temporary.newFolder(); val entered = CountDownLatch(1); val release = CountDownLatch(1)
        val backend = backend(root) { if (it == "catalogue-before-commit") { entered.countDown(); check(release.await(30, TimeUnit.SECONDS)) } }
        var closing: java.util.concurrent.CompletableFuture<Void>? = null
        try {
            note(backend)
            val artifact = prepared(backend)
            val pending = artifact.publish(File(temporary.newFolder(), "selected.stemenc"), true)
            assertTrue(entered.await(15, TimeUnit.SECONDS))
            assertFalse(pending.cancel())
            closing = backend.closeFullyAsync()
            assertFalse(closing.isDone)
            try { backend(root); fail("Duplicate app owner admitted") } catch (_: IllegalStateException) { }
            release.countDown()
            assertTrue(pending.await().catalogueRecorded)
            closing.get(15, TimeUnit.SECONDS)
            val reopened = backend(root)
            try { assertTrue(reopened.backups().status().await().currentAuthoringWasPublished) } finally { finish(reopened) }
        } finally { release.countDown(); if (closing == null) finish(backend) else closing.get(15, TimeUnit.SECONDS) }
    }
}
