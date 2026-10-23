package org.stemnotebook.backend

import android.content.ContextWrapper
import android.net.Uri
import android.os.Messenger
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
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference

@RunWith(JniBackendRunner::class)
@Config(sdk = [35])
class BackendOwnershipTest {
    @get:Rule val temporary = TemporaryFolder()
    private fun platform(root: File) = object : ContextWrapper(RuntimeEnvironment.getApplication()) { override fun getNoBackupFilesDir(): File = root }
    private fun call(backend: NotebookBackend, command: JSONObject): JSONObject {
        val response = JSONObject(backend.request(JSONObject().put("version", 1).put("command", command).toString()).get(5, TimeUnit.SECONDS))
        assertTrue(response.optString("error"), response.getBoolean("ok")); return response.optJSONObject("data") ?: JSONObject()
    }
    private fun note(backend: NotebookBackend) = UUID.fromString(call(backend, JSONObject().put("op", "Create").put("kind", "Note").put("parent", JSONObject.NULL).put("title", "ownership fixture")).getString("id"))
    private fun released(root: File) {
        val until = System.nanoTime() + TimeUnit.SECONDS.toNanos(5)
        while (true) {
            try { LibraryOwnership.acquire(root).close(); return } catch (_: IllegalStateException) { }
            assertTrue(System.nanoTime() < until); Thread.sleep(5)
        }
    }
    @Test(timeout = 20_000) fun duplicateAndAliasedBackendAreRejectedBeforeFactoriesOrCacheCleanup() {
        val root = temporary.newFolder(); val context = platform(root); val backend = NotebookBackend(context)
        val factories = AtomicInteger()
        try {
            val id = note(backend)
            val pending = File(root, "pdf-read-cache/pdf-owned.pending").apply { writeText("active owner's private cache") }
            assertThrows(IllegalStateException::class.java) { NotebookBackend(context, pdfFactory = { _, _ -> factories.incrementAndGet(); error("Duplicate must not construct resources") }) }
            val alias = File(temporary.root, "alias")
            java.nio.file.Files.createSymbolicLink(alias.toPath(), root.toPath())
            assertThrows(IllegalStateException::class.java) { NotebookBackend(platform(alias), pdfFactory = { _, _ -> factories.incrementAndGet(); error("Alias must not construct resources") }) }
            val otherRoot = temporary.newFolder()
            java.nio.file.Files.createSymbolicLink(File(otherRoot, "notebook").toPath(), File(root, "notebook").toPath())
            assertThrows(IllegalArgumentException::class.java) { NotebookBackend(platform(otherRoot), pdfFactory = { _, _ -> factories.incrementAndGet(); error("Aliased library directory must not construct resources") }) }
            released(otherRoot)
            assertEquals(0, factories.get()); assertEquals("active owner's private cache", pending.readText())
            assertThrows(IllegalStateException::class.java) { NativeBridge.open(File(root, "notebook/library.db").absolutePath) }
            assertEquals("ownership fixture", call(backend, JSONObject().put("op", "Read").put("id", id.toString())).getString("title"))
        } finally { backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
        val reopened = NotebookBackend(context)
        try { call(reopened, JSONObject().put("op", "Status")); assertFalse(File(root, "pdf-read-cache/pdf-owned.pending").exists()) }
        finally { reopened.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
    @Test(timeout = 20_000) fun nativeDrainDoesNotReleaseAppOwnershipWhileProviderIsStillRunning() {
        val root = temporary.newFolder(); val context = platform(root)
        val entered = CountDownLatch(1); val release = CountDownLatch(1); val returned = AtomicReference<ParcelFileDescriptor>()
        val source = File(root, "source.pdf").apply { writeText("%PDF-late provider fixture") }
        val provider = PdfProvider({ _, _ -> entered.countDown(); release.await(); ParcelFileDescriptor.open(source, ParcelFileDescriptor.MODE_READ_ONLY).also { returned.set(it) } }, {}, {})
        val backend = NotebookBackend(context, pdfFactory = { platform, native -> PdfWorkspace(platform, native) }, selectionProvider = provider)
        try {
            val id = note(backend)
            val pending = backend.reviewPdfSelection(PdfSelectionManager.Target.Link(id), Uri.parse("content://synthetic/ownership"), 1, 10_000)
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            val cache = File(root, "pdf-retention-cache/retention-owned.pending").apply { writeText("owned until late operation retires") }
            backend.closeAsync().get(5, TimeUnit.SECONDS)
            val stopped = backend.closeFullyAsync(); assertFalse(stopped.isDone)
            assertThrows(CancellationException::class.java) { pending.await() }
            assertThrows(IllegalStateException::class.java) { NotebookBackend(context) }
            assertEquals("owned until late operation retires", cache.readText())
            release.countDown(); stopped.get(5, TimeUnit.SECONDS)
            assertFalse(returned.get().fileDescriptor.valid())
            val reopened = NotebookBackend(context)
            try { assertFalse(cache.exists()); assertEquals("ownership fixture", call(reopened, JSONObject().put("op", "Read").put("id", id.toString())).getString("title")) }
            finally { reopened.closeFullyAsync().get(5, TimeUnit.SECONDS) }
        } finally { release.countDown(); backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
    @Test(timeout = 20_000) fun nativeDrainRetainsOwnershipUntilBlockedParserDisposalFinishes() {
        val root = temporary.newFolder(); val context = platform(root); val cache = File(root, "render-cache").apply { mkdirs() }
        ShadowStatFs.registerStats(cache, 1_000_000, 1_000_000, 1_000_000)
        val entered = CountDownLatch(1); val release = CountDownLatch(1)
        val source = File(root, "source.pdf").apply { writeText("%PDF-parser ownership fixture") }
        val backend = NotebookBackend(context, pdfFactory = { _, native ->
            PdfWorkspace(cache, PdfProvider({ _, _ -> ParcelFileDescriptor.open(source, ParcelFileDescriptor.MODE_READ_ONLY) }, {}, {}), PdfParserClient { callbacks ->
                val fixture = FixturePdfConnection(callbacks, "ok")
                object : PdfConnection {
                    override fun connect(ready: (Messenger) -> Unit, failed: () -> Unit) = fixture.connect(ready, failed)
                    override fun close() { entered.countDown(); release.await(); fixture.close() }
                }
            }, native)
        })
        try {
            val entity = note(backend)
            val digest = java.security.MessageDigest.getInstance("SHA-256").digest(source.readBytes()).joinToString("") { "%02x".format(it.toInt() and 255) }
            val linked = call(backend, JSONObject().put("op", "Pdf").put("command", JSONObject().put("op", "Link").put("context", entity.toString()).put("locator", "content://synthetic/parser").put("revision", digest)))
            val pending = backend.renderPdf(UUID.fromString(linked.getString("id")), 0, 2, 2, 10_000)
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            backend.closeAsync().get(5, TimeUnit.SECONDS)
            assertThrows(CancellationException::class.java) { pending.await() }
            assertFalse(backend.closeFullyAsync().isDone)
            assertThrows(IllegalStateException::class.java) { NotebookBackend(context) }
            release.countDown(); backend.closeFullyAsync().get(5, TimeUnit.SECONDS)
            val reopened = NotebookBackend(context)
            try { call(reopened, JSONObject().put("op", "Status")) } finally { reopened.closeFullyAsync().get(5, TimeUnit.SECONDS) }
        } finally { release.countDown(); backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
    @Test(timeout = 20_000) fun factoryFailureClosesPassedProviderAndDoesNotLeakANativeHandleOrOwnership() {
        val root = temporary.newFolder(); val context = platform(root)
        val provider = PdfProvider({ _, _ -> throw IOException("Unused provider") }, {}, {})
        assertThrows(IOException::class.java) { NotebookBackend(context, pdfFactory = { _, _ -> throw IOException("synthetic factory failure") }, selectionProvider = provider) }
        released(root); assertFalse(File(root, "notebook/library.db").exists())
        assertThrows(IllegalStateException::class.java) { provider.openReadOnly(Uri.parse("content://synthetic/closed")) }
        val backend = NotebookBackend(context)
        try { call(backend, JSONObject().put("op", "Status")) } finally { backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
    @Test(timeout = 20_000) fun failedWorkspaceInitialisationLeavesNoNativeHandleAndCanRecoverAfterFixtureRepair() {
        val root = temporary.newFolder(); val context = platform(root)
        val blocked = File(root, "pdf-read-cache/pdf-blocked.pending").apply { mkdirs() }
        val child = File(blocked, "synthetic-blocker").apply { writeText("cleanup fault") }
        assertThrows(IllegalStateException::class.java) { NotebookBackend(context) }
        released(root); assertFalse(File(root, "notebook/library.db").exists())
        check(child.delete()); check(blocked.delete())
        val backend = NotebookBackend(context)
        try { call(backend, JSONObject().put("op", "Status")) } finally { backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
    @Test(timeout = 20_000) fun failedNativeOpenAutomaticallyRetiresOwnershipAndAllowsExplicitRecovery() {
        val root = temporary.newFolder(); val context = platform(root)
        val database = File(root, "notebook/library.db"); check(database.parentFile!!.mkdirs()); database.writeText("invalid synthetic database")
        val failed = NotebookBackend(context)
        // Calling close also covers the race where the asynchronous open is still running.
        failed.closeFullyAsync().get(5, TimeUnit.SECONDS); released(root)
        database.writeText("")
        val backend = NotebookBackend(context)
        try { call(backend, JSONObject().put("op", "Status")) } finally { backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
}
