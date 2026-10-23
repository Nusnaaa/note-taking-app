package org.stemnotebook.backend

import android.content.Context
import android.os.StatFs
import java.io.Closeable
import java.io.File
import java.io.RandomAccessFile
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.CompletableFuture
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit
import java.util.UUID
import org.json.JSONObject

/** Owns native lifetime and serialises requests away from the Android main thread. */
class NotebookBackend internal constructor(context: Context, pdfFactory: (Context, (JSONObject) -> JSONObject) -> PdfWorkspace, selectionProvider: PdfProvider = PdfProvider(context)) : Closeable {
    constructor(context: Context) : this(context, { platform, native -> PdfWorkspace(platform, native) })
    private val directory = File(context.noBackupFilesDir, "notebook").apply { mkdirs() }
    private val executor = ThreadPoolExecutor(1, 1, 0, TimeUnit.MILLISECONDS, ArrayBlockingQueue(64), ThreadPoolExecutor.AbortPolicy())
    private val handle = CompletableFuture.supplyAsync({ NativeBridge.open(File(directory, "library.db").absolutePath) }, executor)
    private var closed = false
    private val nativeClosed = CompletableFuture<Void>()
    private val reserve = File(directory, "emergency.reserve")
    private val pdf = pdfFactory(context) { command ->
        JSONObject(request(JSONObject().put("version", 1).put("command", command).toString()).join())
    }
    private val selections = PdfSelectionManager(selectionProvider,
        { target, locator, revision, proof -> prepareSelectionNative(target, locator, revision, proof).join() },
        { review, confirmed, proof -> finishSelectionNative(review, confirmed, proof).join() },
        { review -> request(JSONObject().put("version", 1).put("command", JSONObject().put("op", "Pdf").put("command", JSONObject().put("op", "ReleaseSelection").put("review", review.toString()))).toString()); Unit },
        { target ->
            val command = when (target) {
                is PdfSelectionManager.Target.Link -> JSONObject().put("op", "Read").put("id", target.context.toString())
                is PdfSelectionManager.Target.Relink -> JSONObject().put("op", "Pdf").put("command", JSONObject().put("op", "Source").put("source", target.source.toString()))
            }
            val result = JSONObject(request(JSONObject().put("version", 1).put("command", command).toString()).join())
            if (!result.getBoolean("ok")) throw java.io.IOException("PDF selection context unavailable")
        },
    )
    private val outbox = CompletableFuture.supplyAsync({ CaptureOutbox(File(directory, "capture-recovery")) }, executor)

    /** The caller chooses and retains this ticket before submission. Rejection
     * before admission leaves that caller's observations authoritative. After
     * admission, unsuccessful JNI replies remain in the durable recovery journal. */
    @Synchronized fun capture(ticket: UUID, entity: UUID, coordinateSpace: String, strokes: List<CompletedStroke>): CompletableFuture<Int> {
        check(!closed)
        require(strokes.size in 1..1000 && strokes.sumOf { it.points.size.toLong() } <= 100_000)
        // Snapshot mutable caller collections before crossing the thread boundary.
        val snapshots = strokes.map { it.copy(points = it.points.toList()) }
        return CompletableFuture.supplyAsync({
            releaseReserveIfNeeded()
            outbox.join().enqueue(ticket, CaptureRequest.encode(ticket, entity, coordinateSpace, snapshots))
            replayCaptures()
        }, executor)
    }

    /** Call after opening/unlocking and after a recoverable storage failure. A
     * failed first capture stops replay without deleting it or later records. */
    @Synchronized fun retryCaptures(): CompletableFuture<Int> {
        check(!closed)
        return CompletableFuture.supplyAsync({ releaseReserveIfNeeded(); replayCaptures() }, executor)
    }
    @Synchronized fun pendingCaptures(): CompletableFuture<Int> {
        check(!closed)
        return CompletableFuture.supplyAsync({ outbox.join().pendingCount() }, executor)
    }
    private fun replayCaptures(): Int = outbox.join().replay {
        val reply = JSONObject(NativeBridge.request(handle.join(), it))
        reply.getInt("version") == 1 && reply.getBoolean("ok")
    }
    private fun releaseReserveIfNeeded() {
        if (StatFs(directory.path).availableBytes < 8L * 1024 * 1024 && reserve.exists()) check(reserve.delete())
    }

    /** Completion with ok=true is the only commit acknowledgement. Queue rejection
     * leaves the caller responsible for retaining and retrying the same capture ticket. */
    @Synchronized fun request(json: String): CompletableFuture<String> {
        check(!closed)
        require(json.length <= 8 * 1024 * 1024)
        if (affectsPdfPolicy(JSONObject(json).getJSONObject("command"))) { pdf.invalidate(); selections.invalidate() }
        return CompletableFuture.supplyAsync({
            releaseReserveIfNeeded()
            NativeBridge.request(handle.join(), json)
        }, executor)
    }

    @Synchronized fun authenticate(scopesJson: String, proof: NativeBridge.Verification): CompletableFuture<Void> {
        check(!closed)
        pdf.invalidate(); selections.invalidate()
        return CompletableFuture.runAsync({ NativeBridge.authenticate(handle.join(), scopesJson, proof) }, executor)
    }

    @Synchronized fun allocateReserve(bytes: Long = 4L * 1024 * 1024): CompletableFuture<Void> {
        check(!closed)
        return CompletableFuture.runAsync({
        require(bytes in 1..64L * 1024 * 1024)
        if (reserve.length() != bytes) RandomAccessFile(reserve, "rw").use { out ->
            out.setLength(0)
            val buffer = ByteArray(65536)
            var remaining = bytes
            while (remaining > 0) { val count = minOf(remaining, buffer.size.toLong()).toInt(); out.write(buffer, 0, count); remaining -= count }
            out.fd.sync()
        }
        }, executor)
    }

    /** Backend reference rendering only; no Activity/Compose or production H.I. */
    @Synchronized fun renderPdf(source: UUID, page: Int, width: Int, height: Int, timeoutMillis: Long = 30_000): PdfWorkspace.PendingFrame {
        check(!closed)
        return pdf.render(source, page, width, height, timeoutMillis)
    }
    @Synchronized fun renderRetainedPdf(asset: UUID, page: Int, width: Int, height: Int, timeoutMillis: Long = 30_000): PdfWorkspace.PendingFrame {
        check(!closed)
        return pdf.renderAsset(asset, page, width, height, timeoutMillis)
    }
    @Synchronized fun reviewPdfSelection(target: PdfSelectionManager.Target, uri: android.net.Uri, pickerFlags: Int, timeoutMillis: Long = 30_000): PdfSelectionManager.Pending<PdfSelectionManager.Review> {
        check(!closed)
        return selections.review(target, uri, pickerFlags, timeoutMillis)
    }
    @Synchronized fun acceptPdfSelection(review: PdfSelectionManager.Review, timeoutMillis: Long = 30_000): PdfSelectionManager.Pending<PdfSelectionManager.Receipt> {
        check(!closed)
        return selections.accept(review, timeoutMillis)
    }
    @Synchronized private fun prepareSelectionNative(target: String, locator: String, revision: String, proof: NativeBridge.PdfGrant): CompletableFuture<JSONObject> {
        check(!closed)
        return CompletableFuture.supplyAsync({ JSONObject(NativeBridge.preparePdfSelection(handle.join(), target, locator, revision, proof)) }, executor)
    }
    @Synchronized private fun finishSelectionNative(review: String, confirmed: Boolean, proof: NativeBridge.PdfGrant): CompletableFuture<JSONObject> {
        check(!closed)
        pdf.invalidate()
        return CompletableFuture.supplyAsync({ JSONObject(NativeBridge.finishPdfSelection(handle.join(), review, confirmed, proof)) }, executor)
    }
    private fun affectsPdfPolicy(command: JSONObject): Boolean {
        val op = command.getString("op")
        if (op == "Pdf") return command.getJSONObject("command").getString("op") !in setOf(
            "Source", "Link", "SetView", "Anchor", "CreateAnchor", "ReconnectAnchor", "ReviewRevision", "PrepareRead", "ValidateRead", "ReleaseRead", "ReleaseSelection", "Assets", "PrepareAssetRead", "ReadAssetChunk", "ValidateAssetRead", "BeginRetention", "AppendRetention", "FinishRetention", "ReleaseRetention",
        )
        // Conservative for new commands: permissions/containment must never gain
        // a path around preview retirement. Ordinary pen/text edits keep the view.
        return op !in setOf(
            "Block", "Mode", "SetMode", "AppendTo", "CaptureTo", "Append", "Capture", "Edit", "EditRegion", "Partition", "SplitRegion", "MergeRegions",
            "SetHeading", "SetVisible", "SetDisplay", "TransformSelection", "EraseStrokes", "RestoreRevision", "Layout", "SetLayout", "EditCell", "EditDiagramPart",
            "CreateGroup", "Group", "TransformGroup", "Ungroup", "Embed", "CreateEmbed", "EditEmbed", "DuplicateEmbed", "RemoveEmbed", "View", "SaveView",
            "AddLink", "Links", "RemoveLink", "List", "Create", "Read", "Blocks", "Search", "Exercises", "ExerciseBlocks", "SourceInk", "History", "FlatExercises", "SearchViews", "Status",
            "CreateExercise", "RenameExercise", "ExerciseVisibility", "OpenInMaster", "Rename",
        )
    }

    /** Completes after admitted native writer calls drain. Uncooperative platform
     * callbacks may still return late; their private results remain retired. */
    @Synchronized fun closeAsync(): CompletableFuture<Void> { close(); return nativeClosed }

    @Synchronized override fun close() {
        if (closed) return
        closed = true
        pdf.close(); selections.close()
        // Flush accepted requests in order. A separate closer avoids blocking the H.I.
        Thread({
            try {
                executor.shutdown()
                while (!executor.awaitTermination(1, TimeUnit.SECONDS)) { /* wait for durable replies */ }
                if (!handle.isCompletedExceptionally) NativeBridge.close(handle.join())
                nativeClosed.complete(null)
            } catch (error: Exception) { nativeClosed.completeExceptionally(error) }
        }, "stem-close").start()
    }
}
