package org.stemnotebook.backend

import android.content.ContextWrapper
import android.view.InputDevice
import android.view.MotionEvent
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
import java.util.concurrent.CountDownLatch
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.TimeUnit

@RunWith(JniBackendRunner::class)
@Config(sdk = [35])
class BackendAdmissionIntegrationTest {
    @get:Rule val temporary = TemporaryFolder()
    private fun platform(root: File) = object : ContextWrapper(RuntimeEnvironment.getApplication()) { override fun getNoBackupFilesDir(): File = root }
    private fun wire(command: JSONObject) = JSONObject().put("version", 1).put("command", command).toString()
    private fun call(backend: NotebookBackend, command: JSONObject): JSONObject {
        val response = JSONObject(backend.request(wire(command)).get(5, TimeUnit.SECONDS))
        assertTrue(response.optString("error"), response.getBoolean("ok")); return response
    }
    private fun note(backend: NotebookBackend) = UUID.fromString(call(backend, JSONObject().put("op", "Create").put("kind", "Note").put("parent", JSONObject.NULL).put("title", "admission fixture")).getJSONObject("data").getString("id"))
    private fun blocks(entity: UUID) = JSONObject().put("op", "Blocks").put("entity", entity.toString()).put("offset", 0).put("limit", 100)
    private fun stroke() = CapturedStroke(UUID.randomUUID().toString(), UUID.randomUUID().toString(), 0, 7, false, listOf(ObservedPoint(-0.0, 0.125, 0, null, null, null, null)))
    private fun submit(session: JournaledCaptureSession, action: Int, time: Long): JournaledCaptureSession.Submission {
        val property = MotionEvent.PointerProperties().apply { id = 7; toolType = MotionEvent.TOOL_TYPE_STYLUS }
        val coordinate = MotionEvent.PointerCoords().apply { x = 0.125f; y = 3.5f; pressure = 0.5f }
        val event = MotionEvent.obtain(10, time, action, 1, arrayOf(property), arrayOf(coordinate), 0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_STYLUS, 0)
        return try { session.accept(event).single() } finally { event.recycle() }
    }
    @Test(timeout = 20_000) fun independentEmbedLayersCrossActualJniWithoutChangingCanvasDefaults() {
        val backend = NotebookBackend(platform(temporary.newFolder()))
        try {
            val host = note(backend)
            val canvas = call(backend, JSONObject().put("op", "Create").put("kind", "Canvas").put("parent", JSONObject.NULL).put("title", "layers")).getJSONObject("data").getString("id")
            val ids = listOf("one", "two").map { text -> call(backend, JSONObject().put("op", "Append").put("entity", canvas).put("content", JSONObject().put("type", "Text").put("value", text))).getJSONObject("data").getString("id") }
            val layer = JSONObject().put("id", UUID.randomUUID().toString()).put("name", "first").put("visible", true).put("blocks", org.json.JSONArray().put(ids[0]))
            val state = JSONObject().put("canvas", canvas).put("revision", 0).put("base_visible", false).put("layers", org.json.JSONArray().put(layer))
            call(backend, JSONObject().put("op", "SetCanvasLayers").put("state", state).put("expected", 0))
            val embed = call(backend, JSONObject().put("op", "CreateEmbed").put("host", host.toString()).put("source", canvas).put("frame", JSONObject().put("x", 0).put("y", 0).put("width", 100).put("height", 50))).getJSONObject("data").getString("id")
            fun visible(): String {
                val scene = call(backend, JSONObject().put("op", "CanvasScene").put("canvas", canvas).put("embed", embed).put("offset", 0).put("limit", 10)).getJSONObject("data")
                assertEquals(1, scene.getJSONArray("items").length())
                return scene.getJSONArray("items").getJSONObject(0).getJSONObject("item").getJSONObject("Block").getJSONObject("item").getJSONObject("Visible").getString("id")
            }
            assertEquals(ids[0], visible())
            call(backend, JSONObject().put("op", "SetEmbedLayers").put("embed", embed).put("revision", 0).put("visible", org.json.JSONArray().put("Base")))
            assertEquals(ids[1], visible()); assertFalse(call(backend, JSONObject().put("op", "CanvasLayers").put("canvas", canvas)).getJSONObject("data").getBoolean("base_visible"))
            call(backend, JSONObject().put("op", "SetEmbedLayers").put("embed", embed).put("revision", 1).put("visible", JSONObject.NULL)); assertEquals(ids[0], visible())
            call(backend, JSONObject().put("op", "Undo").put("entity", host.toString())); assertEquals(ids[1], visible())
        } finally { backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
    @Test(timeout = 20_000) fun workspaceFindsSavedPaneAndToolAfterFullBackendReopen() {
        val context = platform(temporary.newFolder()); var backend = NotebookBackend(context)
        try {
            val entity = note(backend); val view = UUID.randomUUID(); val slot = UUID.randomUUID()
            val settings = JSONObject().put("id", view.toString()).put("entity", entity.toString()).put("exercise", JSONObject.NULL)
                .put("camera", JSONObject().put("centre_x", 4).put("centre_y", -2).put("zoom", 3).put("rotation_rad", 0.5))
                .put("pen_width_mm", 0.6).put("colour_rgba", org.json.JSONArray(listOf(1, 2, 3, 255))).put("finger_ink", false).put("anchor", JSONObject.NULL).put("revision", 0)
            call(backend, JSONObject().put("op", "SaveView").put("view", settings).put("expected", JSONObject.NULL))
            val pane = JSONObject().put("slot", slot.toString()).put("weight", 3).put("target", JSONObject().put("View", JSONObject().put("view", view.toString()).put("tool", "Eraser")))
            val workspace = JSONObject().put("panes", org.json.JSONArray().put(pane)).put("active", slot.toString()).put("axis", "Vertical")
            val command = JSONObject().put("op", "SaveWorkspace").put("workspace", workspace).put("expected", 0)
            assertEquals(1, call(backend, command).getJSONObject("data").getInt("revision"))
            assertFalse(JSONObject(backend.request(wire(command)).get(5, TimeUnit.SECONDS)).getBoolean("ok"))
            backend.closeFullyAsync().get(5, TimeUnit.SECONDS); backend = NotebookBackend(context)
            val found = call(backend, JSONObject().put("op", "Views").put("entity", entity.toString()).put("exercise", JSONObject.NULL).put("offset", 0).put("limit", 10)).getJSONArray("data")
            assertEquals(1, found.length()); assertEquals(view.toString(), found.getString(0))
            val restored = call(backend, JSONObject().put("op", "RestoreWorkspace")).getJSONObject("data")
            assertEquals(slot.toString(), restored.getString("active")); assertEquals("Vertical", restored.getString("axis"))
            val ready = restored.getJSONArray("panes").getJSONObject(0).getJSONObject("state").getJSONObject("View")
            assertEquals("Eraser", ready.getString("tool")); val saved = ready.getJSONObject("view").getJSONObject("settings")
            assertEquals(view.toString(), saved.getString("id")); assertEquals(3.0, saved.getJSONObject("camera").getDouble("zoom"), 0.0)
        } finally { backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
    @Test(timeout = 20_000) fun savedUnicodeSelectionRestoresOnlyAtItsOriginalRevision() {
        val backend = NotebookBackend(platform(temporary.newFolder())); val entity = note(backend)
        try {
            val block = call(backend, JSONObject().put("op", "Append").put("entity", entity.toString()).put("content", JSONObject().put("type", "Text").put("value", "A😀?"))).getJSONObject("data").getString("id")
            val point = JSONObject().put("block", block).put("revision", 0).put("region", JSONObject.NULL).put("target", JSONObject().put("Inline", JSONObject().put("Text", JSONObject().put("offset", 2))))
            val view = UUID.randomUUID()
            val settings = JSONObject().put("id", view.toString()).put("entity", entity.toString()).put("exercise", JSONObject.NULL)
                .put("camera", JSONObject().put("centre_x", 0).put("centre_y", 0).put("zoom", 1).put("rotation_rad", 0))
                .put("pen_width_mm", 0.5).put("colour_rgba", org.json.JSONArray(listOf(0, 0, 0, 255))).put("finger_ink", false).put("anchor", JSONObject.NULL).put("revision", 0)
                .put("bookmark", JSONObject().put("anchor", point).put("focus", point))
            call(backend, JSONObject().put("op", "SaveView").put("view", settings).put("expected", JSONObject.NULL))
            assertEquals("Current", call(backend, JSONObject().put("op", "RestoreView").put("view", view.toString())).getJSONObject("data").getString("bookmark_state"))
            call(backend, JSONObject().put("op", "Edit").put("block", block).put("revision", 0).put("content", JSONObject().put("type", "Text").put("value", "A😀!")))
            val restored = call(backend, JSONObject().put("op", "RestoreView").put("view", view.toString())).getJSONObject("data")
            assertEquals("NeedsReview", restored.getString("bookmark_state")); assertEquals(0, restored.getJSONObject("settings").getJSONObject("bookmark").getJSONObject("anchor").getInt("revision"))
        } finally { backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
    @Test(timeout = 20_000) fun fullReadLaneStillAdmitsObservedInkAndQueuedReadsSeeItsCommit() {
        val backend = NotebookBackend(platform(temporary.newFolder())); val entity = note(backend)
        val entered = CountDownLatch(1); val release = CountDownLatch(1)
        try {
            val observer = backend.observeWriterStatus(); assertNotNull(observer.next())
            val auth = backend.authenticate("[\"App\"]") { entered.countDown(); release.await(10, TimeUnit.SECONDS) }
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            val reads = List(16) { backend.request(wire(blocks(entity))) }
            assertThrows(RejectedExecutionException::class.java) { backend.request(wire(blocks(entity))) }
            val session = backend.openCaptureSession(entity, "fixed-space")
            val down = submit(session, MotionEvent.ACTION_DOWN, 10); val up = submit(session, MotionEvent.ACTION_UP, 20)
            val pending = backend.writerStatus(); assertEquals(3, pending.pendingAuthoring); assertEquals(16, pending.pendingQueries)
            assertTrue(pending.authoringBytes > 0 && pending.queryBytes > 0); assertEquals(pending, observer.next())
            release.countDown(); auth.get(5, TimeUnit.SECONDS)
            assertTrue(down.receipt.get(5, TimeUnit.SECONDS).checkpointed); assertTrue(up.receipt.get(5, TimeUnit.SECONDS).committed)
            reads.forEach { assertEquals(1, JSONObject(it.get(5, TimeUnit.SECONDS)).getJSONArray("data").length()) }
            assertEquals(0, backend.writerStatus().pendingAuthoring); assertEquals(0, backend.writerStatus().pendingQueries)
            assertFalse(backend.writerStatus().needsAttention)
        } finally { release.countDown(); backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
        assertTrue(backend.writerStatus().closed); assertTrue(backend.writerStatus().terminated)
    }
    @Test(timeout = 20_000) fun authoringRejectionRetainsCallerFrameForExactRetryAfterCapacityRecovers() {
        val backend = NotebookBackend(platform(temporary.newFolder())); val entity = note(backend)
        val entered = CountDownLatch(1); val release = CountDownLatch(1)
        try {
            val running = backend.authenticate("[\"App\"]") { entered.countDown(); release.await(10, TimeUnit.SECONDS) }
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            val queued = List(63) { backend.authenticate("[\"App\"]") { true } }
            val session = backend.openCaptureSession(entity, "fixed-space")
            val down = submit(session, MotionEvent.ACTION_DOWN, 10)
            val failure = assertThrows(java.util.concurrent.ExecutionException::class.java) { down.receipt.get(5, TimeUnit.SECONDS) }
            assertTrue(failure.cause is RejectedExecutionException); assertEquals(1, down.stroke.points.size)
            assertEquals(64, backend.writerStatus().pendingAuthoring)
            release.countDown(); running.get(5, TimeUnit.SECONDS); queued.forEach { it.get(5, TimeUnit.SECONDS) }
            assertTrue(session.retry(down).receipt.get(5, TimeUnit.SECONDS).checkpointed)
            val up = submit(session, MotionEvent.ACTION_UP, 20); assertEquals(down.stroke.id, up.stroke.id)
            assertTrue(up.receipt.get(5, TimeUnit.SECONDS).committed)
            assertEquals(1, call(backend, blocks(entity)).getJSONArray("data").length())
        } finally { release.countDown(); backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
    @Test(timeout = 20_000) fun queuedInputBytesExhaustBeforeCountAndCannotBorrowCaptureCapacity() {
        val backend = NotebookBackend(platform(temporary.newFolder())); val entity = note(backend)
        val entered = CountDownLatch(1); val release = CountDownLatch(1)
        try {
            val auth = backend.authenticate("[\"App\"]") { entered.countDown(); release.await(10, TimeUnit.SECONDS) }
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            val large = wire(JSONObject().put("op", "Search").put("query", "x".repeat(1_100_000)))
            val read = backend.request(large)
            assertThrows(RejectedExecutionException::class.java) { backend.request(large) }
            assertEquals(1, backend.writerStatus().pendingQueries); assertTrue(backend.writerStatus().queryBytes > 4L * 1024 * 1024)
            val capture = backend.capture(UUID.randomUUID(), entity, "fixed-space", listOf(stroke()))
            release.countDown(); auth.get(5, TimeUnit.SECONDS); assertEquals(1, capture.get(5, TimeUnit.SECONDS).toInt()); assertFalse(JSONObject(read.get(5, TimeUnit.SECONDS)).getBoolean("ok"))
            assertEquals(0L, backend.writerStatus().queryBytes); assertFalse(backend.writerStatus().needsAttention)
            assertEquals(1, call(backend, blocks(entity)).getJSONArray("data").length())
            assertThrows(IllegalArgumentException::class.java) { backend.authenticate("x".repeat(65537)) { true } }
        } finally { release.countDown(); backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
    @Test(timeout = 20_000) fun captureAndRelockKeepFifoAndQueuedReadChecksCurrentAuthority() {
        val backend = NotebookBackend(platform(temporary.newFolder())); val entity = note(backend)
        val scopes = "[{\"Entity\":\"$entity\"}]"; val entered = CountDownLatch(1); val release = CountDownLatch(1)
        try {
            backend.authenticate(scopes) { true }.get(5, TimeUnit.SECONDS)
            call(backend, JSONObject().put("op", "SetLock").put("entity", entity.toString()).put("revision", 0).put("locked", true))
            call(backend, JSONObject().put("op", "Relock"))
            val auth = backend.authenticate(scopes) { entered.countDown(); release.await(10, TimeUnit.SECONDS) }
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            val read = backend.request(wire(blocks(entity)))
            val first = backend.capture(UUID.randomUUID(), entity, "fixed-space", listOf(stroke()))
            val relock = backend.request(wire(JSONObject().put("op", "Relock")))
            val later = backend.capture(UUID.randomUUID(), entity, "fixed-space", listOf(stroke()))
            release.countDown(); auth.get(5, TimeUnit.SECONDS)
            assertEquals(1, first.get(5, TimeUnit.SECONDS).toInt()); assertTrue(JSONObject(relock.get(5, TimeUnit.SECONDS)).getBoolean("ok"))
            assertEquals(0, later.get(5, TimeUnit.SECONDS).toInt()); assertFalse(JSONObject(read.get(5, TimeUnit.SECONDS)).getBoolean("ok"))
            assertEquals(1, backend.pendingCaptures().get(5, TimeUnit.SECONDS).toInt()); assertTrue(backend.writerStatus().needsAttention)
            backend.authenticate(scopes) { true }.get(5, TimeUnit.SECONDS)
            assertEquals(2, call(backend, blocks(entity)).getJSONArray("data").length()); assertTrue(backend.writerStatus().needsAttention)
        } finally { release.countDown(); backend.closeFullyAsync().get(5, TimeUnit.SECONDS) }
    }
}
