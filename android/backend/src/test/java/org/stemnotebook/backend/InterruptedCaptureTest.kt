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
import java.util.UUID
import java.util.concurrent.TimeUnit

@RunWith(JniBackendRunner::class)
@Config(sdk = [35])
class InterruptedCaptureTest {
    @get:Rule val temporary = TemporaryFolder()
    private fun call(backend: NotebookBackend, command: JSONObject): JSONObject {
        val result = JSONObject(backend.request(JSONObject().put("version", 1).put("command", command).toString()).get(5, TimeUnit.SECONDS))
        assertTrue(result.optString("error"), result.getBoolean("ok")); return result
    }
    @Test(timeout = 20_000) fun interruptedObservedPrefixPersistsThroughActualJniRetryAndReopenWithItsEndProvenance() {
        val root = temporary.newFolder()
        val context = object : ContextWrapper(RuntimeEnvironment.getApplication()) { override fun getNoBackupFilesDir(): File = root }
        val backend = NotebookBackend(context)
        val accumulator = InkAccumulator(); accumulator.begin(7, false, ObservedPoint(-0.0, 1.2345678901234567, 10, 0.0, null, null, 0))
        val prefix = accumulator.checkpoint()
        val note = UUID.fromString(call(backend, JSONObject().put("op", "Create").put("kind", "Note").put("parent", JSONObject.NULL).put("title", "interrupted capture")).getJSONObject("data").getString("id"))
        val ticket = UUID.randomUUID(); val sealed = accumulator.interrupt(StrokeInterruption.Lifecycle)
        assertEquals(prefix.single().id, sealed.single().id)
        var block: String? = null
        try {
            assertEquals(1, backend.capture(ticket, note, "fixed-space", sealed).get(5, TimeUnit.SECONDS).toInt())
            assertEquals(1, backend.capture(ticket, note, "fixed-space", sealed).get(5, TimeUnit.SECONDS).toInt())
            val blocks = call(backend, JSONObject().put("op", "Blocks").put("entity", note.toString()).put("offset", 0).put("limit", 10)).getJSONArray("data")
            assertEquals(1, blocks.length()); block = blocks.getJSONObject(0).getString("id")
        } finally { backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
        val reopened = NotebookBackend(context)
        try {
            val stroke = call(reopened, JSONObject().put("op", "SourceInk").put("block", block)).getJSONArray("data").getJSONObject(0)
            assertEquals(prefix.single().id, stroke.getString("id"))
            assertEquals("Lifecycle", stroke.getJSONObject("end").getString("Interrupted"))
            val points = stroke.getJSONArray("points"); assertEquals(1, points.length())
            val point = points.getJSONObject(0); assertEquals(10L, point.getLong("time_us"))
            assertEquals((-0.0).toRawBits(), point.getDouble("x").toRawBits()); assertEquals(1.2345678901234567.toRawBits(), point.getDouble("y").toRawBits())
            assertEquals(0.0, point.getDouble("pressure"), 0.0); assertTrue(point.isNull("tilt_rad"))
        } finally { reopened.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
}
