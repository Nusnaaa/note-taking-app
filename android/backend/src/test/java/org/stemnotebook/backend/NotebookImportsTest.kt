package org.stemnotebook.backend

import android.content.ContextWrapper
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
import java.util.UUID
import java.util.concurrent.Callable
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

/** Actual desktop JNI in API35 simulation with app-owned synthetic providers. */
@RunWith(JniBackendRunner::class)
@Config(sdk = [35])
class NotebookImportsTest {
    @get:Rule val temporary = TemporaryFolder()
    private val document = Uri.parse("content://synthetic-import/selected")
    private fun <T> observer(action: () -> T): T {
        val executor = Executors.newSingleThreadExecutor()
        return try { executor.submit(Callable { action() }).get(180, TimeUnit.SECONDS) }
        finally { executor.shutdownNow() }
    }
    private fun native(host: Long, command: String): JSONObject {
        val result = JSONObject(NativeBridge.request(host,"""{"version":1,"command":$command}"""))
        assertTrue(result.optString("error"), result.getBoolean("ok")); return result
    }
    private fun failure(action: () -> Unit): NotebookImports.Failure {
        try { action(); throw AssertionError("invalid import accepted") }
        catch (error: NotebookImports.Failure) { assertEquals("Native import operation unavailable",error.message); return error }
    }
    private data class Artifact(val bytes: ByteArray, val key: CharArray?, val original: UUID)
    private fun artifact(protected: Boolean = false, password: CharArray? = null): Artifact {
        val host = NativeBridge.open(temporary.newFolder().resolve("source.db").absolutePath)
        try {
            val entity = UUID.fromString(native(host,"""{"op":"Create","kind":"Note","parent":null,"title":"import λ"}""").getJSONObject("data").getString("id"))
            native(host,"""{"op":"Append","entity":"$entity","content":{"type":"Text","value":"001 + 1 = 3"}}""")
            native(host,"""{"op":"Capture","ticket":"${UUID.randomUUID()}","entity":"$entity","strokes":[{"id":"${UUID.randomUUID()}","capture_session":"${UUID.randomUUID()}","sequence":0,"pointer_id":1,"coordinate_space":"synthetic-import-units","tool":"Pen","points":[{"x":1.2345678901234567,"y":2.0,"time_us":1,"pressure":null,"tilt_rad":null,"orientation_rad":null,"buttons":null}]}]}""")
            if (protected) {
                NativeBridge.authenticate(host,"""[{"Entity":"$entity"}]""") { true }
                val revision = native(host,"""{"op":"Read","id":"$entity"}""").getJSONObject("data").getLong("revision")
                native(host,"""{"op":"SetLock","entity":"$entity","revision":$revision,"locked":true}""")
            }
            NativeBridge.reviewNativeExport(host,"""{"Entities":["$entity"]}""",true).use { review ->
                review.finish(password,{ true },true,true,120000).use { data -> return Artifact(data.bytes.clone(),data.recoveryKey?.clone(),entity) }
            }
        } finally { NativeBridge.close(host) }
    }
    private open class Documents(val bytes: ByteArray) : ImportDocuments() {
        @Volatile var allowed = true
        val opens = AtomicInteger()
        val closed = AtomicInteger()
        override fun validate(document: Uri) {
            assertEquals("stem-imports",Thread.currentThread().name)
            if (!allowed) throw IOException("private provider diagnostic")
        }
        override fun open(document: Uri, signal: CancellationSignal): InputStream {
            validate(document); opens.incrementAndGet()
            return object : ByteArrayInputStream(bytes) { override fun close() { closed.incrementAndGet(); super.close() } }
        }
    }
    private fun shutdown(imports: NotebookImports, host: Long) {
        imports.close(); observer { imports.awaitClosed() }; NativeBridge.close(host)
    }
    @Test fun selectedNativeCopyPreservesExactObservationsAndReconcilesAfterReopen() {
        val source = artifact(); val documents = Documents(source.bytes)
        val path = temporary.newFolder().resolve("library.db").absolutePath
        var host = NativeBridge.open(path)
        var imports = NotebookImports({ host }, documents)
        try {
            val pending = imports.prepare(document,null,false,NotebookImports.Limits(source.bytes.size,30000))
            failure { pending.await() }
            val review = observer { pending.await() }
            assertEquals(pending.operationId,review.operationId)
            assertEquals(1L,review.summary.entities); assertEquals(2L,review.summary.blocks); assertEquals(1L,review.summary.strokes)
            try { (review.summary.sourceOmissions as MutableList<String>).add("mutable"); fail() } catch (_: UnsupportedOperationException) { }
            val receipt = observer { review.publish({ true },true,true).await() }
            assertEquals(review.operationId,receipt.operation); assertNotEquals(source.original,receipt.entities.single())
            val blocks = native(host,"""{"op":"Blocks","entity":"${receipt.entities.single()}","offset":0,"limit":10}""").getJSONArray("data")
            var sawInk = false
            for (index in 0 until blocks.length()) {
                val block = blocks.getJSONObject(index)
                if (block.getJSONObject("content").getString("type") == "Ink") {
                    val ink = native(host,"""{"op":"SourceInk","block":"${block.getString("id")}"}""").getJSONArray("data").getJSONObject(0)
                    val point = ink.getJSONArray("points").getJSONObject(0)
                    assertEquals(1.2345678901234567,point.getDouble("x"),0.0); assertTrue(point.isNull("pressure")); sawInk = true
                }
            }
            assertTrue(sawInk); assertTrue(blocks.toString().contains("001 + 1 = 3")); assertEquals(1,documents.opens.get()); assertEquals(1,documents.closed.get())
            failure { review.publish({ true },true,true) }
            shutdown(imports,host)
            host = NativeBridge.openExisting(path); imports = NotebookImports({ host },documents)
            assertEquals(receipt,observer { imports.reconcile(receipt.operation).await() }.receipt)
        } finally { shutdown(imports,host); source.bytes.fill(0) }
    }
    @Test fun confirmationPermissionAndGenerationChangesNeverSilentlyDuplicateAnImport() {
        val source = artifact(); val documents = Documents(source.bytes)
        val host = NativeBridge.open(temporary.newFolder().resolve("library.db").absolutePath)
        val imports = NotebookImports({ host },documents)
        try {
            var review = observer { imports.prepare(document,null,false,NotebookImports.Limits(source.bytes.size,30000)).await() }
            assertFalse(failure { review.publish({ true },false,true) }.publicationMayHaveCommitted)
            documents.allowed = false
            val denied = observer { failure { review.publish({ true },true,true).await() } }
            assertFalse(denied.publicationMayHaveCommitted); assertEquals(review.operationId,denied.operationId)
            assertNull(observer { imports.reconcile(review.operationId).await() }.receipt)
            documents.allowed = true
            review = observer { imports.prepare(document,null,false,NotebookImports.Limits(source.bytes.size,30000)).await() }
            native(host,"""{"op":"Create","kind":"Note","parent":null,"title":"changed"}""")
            assertTrue(observer { failure { review.publish({ true },true,true).await() } }.publicationMayHaveCommitted)
            assertNull(observer { imports.reconcile(review.operationId).await() }.receipt)
            val tiny = observer { failure { imports.prepare(document,null,false,NotebookImports.Limits(4,30000)).await() } }
            assertEquals(NotebookImports.FailureReason.InvalidInput,tiny.reason); assertFalse(tiny.publicationMayHaveCommitted)
            review = observer { imports.prepare(document,null,false,NotebookImports.Limits(source.bytes.size,30000)).await() }
            imports.invalidate()
            failure { review.publish({ true },true,true) }
            assertNull(observer { imports.reconcile(review.operationId).await() }.receipt)
        } finally { shutdown(imports,host); source.bytes.fill(0) }
    }
    @Test fun encryptedRecoveryPreservesLocksAndCallerOwnedCredentialArrays() {
        val password = "synthetic Kotlin explicit import password".toCharArray()
        val source = artifact(true,password); val documents = Documents(source.bytes)
        val host = NativeBridge.open(temporary.newFolder().resolve("library.db").absolutePath)
        val imports = NotebookImports({ host },documents)
        val key = requireNotNull(source.key); val original = key.clone()
        try {
            assertFalse(observer { failure { imports.prepare(document,"wrong".toCharArray(),false,NotebookImports.Limits(source.bytes.size,120000)).await() } }.publicationMayHaveCommitted)
            val review = observer { imports.prepare(document,key,true,NotebookImports.Limits(source.bytes.size,120000)).await() }
            assertArrayEquals(original,key); assertEquals(1L,review.summary.protectedObjects)
            val receipt = observer { review.publish({ true },true,true).await() }
            assertFalse(JSONObject(NativeBridge.request(host,"""{"version":1,"command":{"op":"Read","id":"${receipt.entities.single()}"}}""")).getBoolean("ok"))
        } finally { shutdown(imports,host); source.bytes.fill(0); key.fill('\u0000'); original.fill('\u0000'); password.fill('\u0000') }
    }
    @Test fun blockedProviderCancellationRetainsBackendOwnershipUntilActualRetirement() {
        val root = temporary.newFolder()
        val context = object : ContextWrapper(RuntimeEnvironment.getApplication()) { override fun getNoBackupFilesDir(): File = root }
        val entered = CountDownLatch(1); val closeStarted = CountDownLatch(1); val allowClose = CountDownLatch(1); val readDone = CountDownLatch(1)
        val provider = object : ImportDocuments() {
            override fun validate(document: Uri) { assertEquals("stem-imports",Thread.currentThread().name) }
            override fun open(document: Uri, signal: CancellationSignal): InputStream = object : InputStream() {
                override fun read(): Int { entered.countDown(); check(readDone.await(120,TimeUnit.SECONDS)); throw IOException("cancelled input") }
                override fun read(bytes: ByteArray,off: Int,len: Int): Int = read()
                override fun close() { closeStarted.countDown(); check(allowClose.await(120,TimeUnit.SECONDS)); readDone.countDown() }
            }
        }
        val backend = NotebookBackend(context,{ platform,native -> PdfWorkspace(platform,native) },importDocuments = provider)
        try {
            val imports = backend.imports()
            val pending = imports.prepare(document,null,false,NotebookImports.Limits(4,120000))
            assertTrue(entered.await(30,TimeUnit.SECONDS)); assertNotNull(pending.operationId)
            val queued = imports.reconcile(UUID.randomUUID())
            assertEquals(NotebookImports.FailureReason.Busy,failure { imports.reconcile(UUID.randomUUID()) }.reason)
            queued.cancel()
            val closed = backend.closeFullyAsync()
            assertTrue(closeStarted.await(30,TimeUnit.SECONDS)); assertFalse(closed.isDone)
            try { NotebookBackend(context); fail("library ownership released early") } catch (_: IllegalStateException) { }
            allowClose.countDown()
            observer { failure { pending.await() } }; observer { failure { queued.await() } }
            closed.get(30,TimeUnit.SECONDS)
            val reopened = NotebookBackend(context); reopened.closeFullyAsync().get(30,TimeUnit.SECONDS)
        } finally { allowClose.countDown(); readDone.countDown(); backend.closeFullyAsync().get(30,TimeUnit.SECONDS) }
    }
    @Test fun deadlineCancelsBlockedInputAndRetiresItsNativeReservation() {
        val host = NativeBridge.open(temporary.newFolder().resolve("library.db").absolutePath)
        val entered = CountDownLatch(1); val cancelled = CountDownLatch(1); val closed = CountDownLatch(1)
        val documents = object : ImportDocuments() {
            override fun validate(document: Uri) { assertEquals("stem-imports",Thread.currentThread().name) }
            override fun open(document: Uri,signal: CancellationSignal): InputStream {
                signal.setOnCancelListener { cancelled.countDown() }
                return object : InputStream() {
                    override fun read(): Int { entered.countDown(); check(cancelled.await(30,TimeUnit.SECONDS)); throw IOException("cancelled read") }
                    override fun read(bytes: ByteArray,off: Int,len: Int): Int = read()
                    override fun close() { closed.countDown() }
                }
            }
        }
        val imports = NotebookImports({ host },documents)
        try {
            val pending = imports.prepare(document,null,false,NotebookImports.Limits(4,5000))
            assertTrue(entered.await(30,TimeUnit.SECONDS))
            val failure = observer { failure { pending.await() } }
            assertEquals(NotebookImports.FailureReason.Cancelled,failure.reason); assertFalse(failure.publicationMayHaveCommitted)
            assertEquals(0L,cancelled.count); assertEquals(0L,closed.count)
            assertNull(observer { imports.reconcile(requireNotNull(pending.operationId)).await() }.receipt)
            NativeBridge.beginNativeImport(host).close()
        } finally { cancelled.countDown(); shutdown(imports,host) }
    }
    @Test fun closingPreparationRetiresReviewWhileCommittedReceiptStaysObservable() {
        val source = artifact(); val documents = Documents(source.bytes)
        val host = NativeBridge.open(temporary.newFolder().resolve("library.db").absolutePath)
        val imports = NotebookImports({ host },documents)
        try {
            val preparation = imports.prepare(document,null,false,NotebookImports.Limits(source.bytes.size,30000))
            val abandoned = observer { preparation.await() }; preparation.close()
            assertEquals(NotebookImports.FailureReason.InvalidInput,failure { abandoned.publish({ true },true,true) }.reason)
            val next = observer { imports.prepare(document,null,false,NotebookImports.Limits(source.bytes.size,30000)).await() }
            val publication = next.publish({ true },true,true)
            val receipt = observer { publication.await() }; publication.close()
            assertEquals(receipt,observer { publication.await() })
            assertEquals(receipt,observer { imports.reconcile(next.operationId).await() }.receipt)
        } finally { shutdown(imports,host); source.bytes.fill(0) }
    }
    @Test fun lostTypedResponseReconcilesTheOriginalOperationWithoutReimport() {
        val source = artifact(); val documents = Documents(source.bytes)
        val host = NativeBridge.open(temporary.newFolder().resolve("library.db").absolutePath)
        val imports = NotebookImports({ host },documents, fault = { if (it == "publication-response") throw IOException("private lost response") })
        try {
            val review = observer { imports.prepare(document,null,false,NotebookImports.Limits(source.bytes.size,30000)).await() }
            val lost = observer { failure { review.publish({ true },true,true).await() } }
            assertTrue(lost.publicationMayHaveCommitted); assertEquals(review.operationId,lost.operationId)
            val receipt = requireNotNull(observer { imports.reconcile(review.operationId).await() }.receipt)
            assertEquals(review.operationId,receipt.operation); assertNotEquals(source.original,receipt.entities.single())
            assertEquals(receipt,observer { imports.reconcile(review.operationId).await() }.receipt)
        } finally { shutdown(imports,host); source.bytes.fill(0) }
    }
    @Test fun appProofRunsOffWriterAndKnownReceiptSurvivesLateControllerClose() {
        val source = artifact(); val documents = Documents(source.bytes)
        val host = NativeBridge.open(temporary.newFolder().resolve("library.db").absolutePath)
        lateinit var imports: NotebookImports
        imports = NotebookImports({ host },documents, fault = { if (it == "publication-response") imports.close() })
        try {
            NativeBridge.authenticate(host,"[\"App\"]") { true }; native(host,"""{"op":"SetAppLock","enabled":true}""")
            var review = observer { imports.prepare(document,null,false,NotebookImports.Limits(source.bytes.size,30000)).await() }
            assertTrue(observer { failure { review.publish({ false },true,true).await() } }.publicationMayHaveCommitted)
            review = observer { imports.prepare(document,null,false,NotebookImports.Limits(source.bytes.size,30000)).await() }
            val publication = review.publish({
                assertEquals("stem-imports",Thread.currentThread().name)
                assertEquals(36,NativeBridge.captureLibraryIdentity(host).length)
                assertEquals(NotebookImports.FailureReason.Unavailable,failure { imports.awaitClosed() }.reason)
                true
            },true,true)
            val receipt = observer { publication.await() }; imports.close()
            assertEquals(receipt,observer { publication.await() }); observer { imports.awaitClosed() }
            val another = NotebookImports({ host },documents)
            try { assertEquals(receipt,observer { another.reconcile(review.operationId).await() }.receipt) }
            finally { another.close(); observer { another.awaitClosed() } }
        } finally { shutdown(imports,host); source.bytes.fill(0) }
    }
}
