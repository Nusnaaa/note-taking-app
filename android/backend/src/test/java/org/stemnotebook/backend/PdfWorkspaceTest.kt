package org.stemnotebook.backend

import android.content.Context
import android.content.ContextWrapper
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.RectF
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
import java.security.MessageDigest
import java.util.UUID
import java.util.concurrent.CancellationException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

@RunWith(JniBackendRunner::class)
@Config(sdk = [35])
class PdfWorkspaceTest {
    @get:Rule val temporary = TemporaryFolder()
    private fun digest(bytes: ByteArray) = MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it.toInt() and 255) }
    private fun command(backend: NotebookBackend, command: JSONObject): JSONObject {
        val result = JSONObject(backend.request(JSONObject().put("version", 1).put("command", command).toString()).get(5, TimeUnit.SECONDS))
        assertTrue(result.optString("error"), result.getBoolean("ok")); return result
    }
    private fun pdf(op: String, fields: JSONObject) = JSONObject().put("op", "Pdf").put("command", fields.put("op", op))
    private fun eventually(condition: () -> Boolean) {
        val until = System.nanoTime() + TimeUnit.SECONDS.toNanos(5)
        while (!condition() && System.nanoTime() < until) Thread.sleep(5)
        assertTrue(condition())
    }
    private class Fixture(val backend: NotebookBackend, val source: File, val cache: File, val parserCalls: AtomicInteger, val context: UUID, val reference: UUID) : AutoCloseable {
        override fun close() { backend.closeAsync().get(5, TimeUnit.SECONDS) }
    }
    private fun fixture(denySecondOpen: Boolean = false, intercept: (JSONObject) -> Unit = {}): Fixture {
        val root = temporary.newFolder()
        val cache = File(root, "cache").apply { mkdirs() }
        ShadowStatFs.registerStats(cache, 1_000_000, 1_000_000, 1_000_000)
        ShadowStatFs.registerStats(File(root, "notebook"), 1_000_000, 1_000_000, 1_000_000)
        val source = File(root, "synthetic.pdf").apply { writeText("%PDF-synthetic revision fixture") }
        val context = object : ContextWrapper(RuntimeEnvironment.getApplication()) { override fun getNoBackupFilesDir(): File = root }
        val calls = AtomicInteger(0)
        val opens = AtomicInteger(0)
        val backend = NotebookBackend(context) { _, native ->
            PdfWorkspace(cache, PdfProvider({ _, _ ->
                if (denySecondOpen && opens.incrementAndGet() > 1) throw SecurityException("Synthetic permission loss")
                ParcelFileDescriptor.open(source, ParcelFileDescriptor.MODE_READ_ONLY)
            }, {}, {}), PdfParserClient { callbacks ->
                calls.incrementAndGet(); FixturePdfConnection(callbacks, "ok")
            }) { request -> intercept(request); native(request) }
        }
        val note = command(backend, JSONObject().put("op", "Create").put("kind", "Note").put("parent", JSONObject.NULL).put("title", "synthetic PDF study"))
        val entity = UUID.fromString(note.getJSONObject("data").getString("id"))
        val linked = command(backend, pdf("Link", JSONObject().put("context", entity.toString()).put("locator", "content://synthetic/study").put("revision", digest(source.readBytes()))))
        return Fixture(backend, source, cache, calls, entity, UUID.fromString(linked.getJSONObject("data").getString("id")))
    }
    @Test(timeout = 20_000) fun actualJniPipelinePreservesInkAndRetiresPreviewBeforeRelockOrRevocation() {
        fixture().use { f ->
            val frame = f.backend.renderPdf(f.reference, 0, 2, 2, 10_000).await()
            assertFalse(frame.retired); assertEquals(3, frame.pages)
            assertEquals(digest(f.source.readBytes()), frame.sourceRevision)
            assertTrue(f.cache.listFiles()!!.isEmpty())
            val observed = ObservedPoint(-0.0, 3.125, 2, 0.0, null, null, null)
            val stroke = CompletedStroke(UUID.randomUUID().toString(), UUID.randomUUID().toString(), 0, 1, false, listOf(observed))
            assertEquals(1, f.backend.capture(UUID.randomUUID(), f.context, "synthetic-space", listOf(stroke)).get(5, TimeUnit.SECONDS).toInt())
            assertFalse(frame.retired)
            val blocks = command(f.backend, JSONObject().put("op", "Blocks").put("entity", f.context.toString()).put("offset", 0).put("limit", 10)).getJSONArray("data")
            val ink = command(f.backend, JSONObject().put("op", "SourceInk").put("block", blocks.getJSONObject(0).getString("id"))).getJSONArray("data")
            assertEquals((-0.0).toRawBits(), ink.getJSONObject(0).getJSONArray("points").getJSONObject(0).getDouble("x").toRawBits())
            command(f.backend, JSONObject().put("op", "Relock"))
            assertTrue(frame.retired)
            val target = Bitmap.createBitmap(2, 2, Bitmap.Config.ARGB_8888)
            try { assertThrows(IllegalStateException::class.java) { frame.draw(Canvas(target), RectF(0f, 0f, 2f, 2f)) } } finally { target.recycle() }
            val refreshed = f.backend.renderPdf(f.reference, 0, 2, 2, 10_000).await()
            command(f.backend, pdf("Revoke", JSONObject().put("source", f.reference.toString())))
            assertTrue(refreshed.retired)
            assertThrows(IOException::class.java) { f.backend.renderPdf(f.reference, 0, 2, 2, 10_000).await() }
            assertEquals(1, command(f.backend, JSONObject().put("op", "Blocks").put("entity", f.context.toString()).put("offset", 0).put("limit", 10)).getJSONArray("data").length())
        }
    }
    @Test(timeout = 20_000) fun changedProviderRevisionNeverReachesParserAndLeavesNoReadCache() {
        fixture().use { f ->
            f.source.writeText("%PDF-changed external source")
            assertThrows(IOException::class.java) { f.backend.renderPdf(f.reference, 0, 2, 2, 10_000).await() }
            eventually { f.cache.listFiles()!!.isEmpty() }
            assertEquals(0, f.parserCalls.get())
        }
    }
    @Test(timeout = 20_000) fun revocationDuringFinalNativeValidationCannotPublishLatePixels() {
        val validations = AtomicInteger(0)
        val entered = CountDownLatch(1)
        val release = CountDownLatch(1)
        val fixture = fixture { request ->
            if (request.getJSONObject("command").getString("op") == "ValidateRead" && validations.incrementAndGet() == 2) {
                entered.countDown(); release.await()
            }
        }
        try {
            val pending = fixture.backend.renderPdf(fixture.reference, 0, 2, 2, 10_000)
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            command(fixture.backend, pdf("Revoke", JSONObject().put("source", fixture.reference.toString())))
            assertThrows(CancellationException::class.java) { pending.await() }
            release.countDown()
            eventually { fixture.cache.listFiles()!!.isEmpty() }
            command(fixture.backend, JSONObject().put("op", "Append").put("entity", fixture.context.toString()).put("content", JSONObject().put("type", "Text").put("value", "independent work remains usable")))
        } finally { release.countDown(); fixture.close() }
    }
    @Test(timeout = 20_000) fun oldUnclaimedReceiptCannotRetireTheNewerFrame() {
        fixture().use { f ->
            val old = f.backend.renderPdf(f.reference, 0, 2, 2, 10_000)
            eventually { old.isReady }
            val fresh = f.backend.renderPdf(f.reference, 1, 2, 2, 10_000).await()
            old.cancel()
            assertFalse(fresh.retired)
            assertThrows(CancellationException::class.java) { old.await() }
        }
    }
    @Test(timeout = 20_000) fun lowSpaceStopsDisposablePdfCopyWithoutModifyingSourceOrNotes() {
        fixture().use { f ->
            val source = f.source.readBytes()
            ShadowStatFs.registerStats(f.cache, 1, 0, 0)
            assertThrows(IOException::class.java) { f.backend.renderPdf(f.reference, 0, 2, 2, 10_000).await() }
            assertArrayEquals(source, f.source.readBytes()); assertEquals(0, f.parserCalls.get())
            command(f.backend, JSONObject().put("op", "Append").put("entity", f.context.toString()).put("content", JSONObject().put("type", "Text").put("value", "still editable")))
        }
    }
    @Test(timeout = 20_000) fun providerPermissionLossBeforePublicationDiscardsAlreadyParsedPixels() {
        fixture(denySecondOpen = true).use { f ->
            assertThrows(IOException::class.java) { f.backend.renderPdf(f.reference, 0, 2, 2, 10_000).await() }
            eventually { f.cache.listFiles()!!.isEmpty() }
            assertEquals(1, f.parserCalls.get())
            command(f.backend, JSONObject().put("op", "Append").put("entity", f.context.toString()).put("content", JSONObject().put("type", "Text").put("value", "independent notes remain usable")))
        }
    }
    @Test fun frameDrawingChecksMonotonicNativeValidityDeadline() {
        val bitmap = Bitmap.createBitmap(2, 2, Bitmap.Config.ARGB_8888)
        val frame = PdfWorkspace.Frame(bitmap, 1, "synthetic-revision", System.nanoTime() - 1)
        assertTrue(frame.retired)
        val target = Bitmap.createBitmap(2, 2, Bitmap.Config.ARGB_8888)
        try { assertThrows(IllegalStateException::class.java) { frame.draw(Canvas(target), RectF()) } } finally { target.recycle(); frame.close() }
    }
}
