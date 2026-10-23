package org.stemnotebook.backend

import android.content.Context
import android.os.StatFs
import java.io.Closeable
import java.io.File
import java.io.RandomAccessFile
import java.util.concurrent.CompletableFuture
import java.util.UUID
import org.json.JSONObject

/** Owns native lifetime and serialises requests away from the Android main thread. */
class NotebookBackend internal constructor(context: Context, pdfFactory: (Context, (JSONObject) -> JSONObject) -> PdfWorkspace, selectionProvider: PdfProvider = PdfProvider(context)) : Closeable {
    constructor(context: Context) : this(context, { platform, native -> PdfWorkspace(platform, native) })
    private val ownership = try { LibraryOwnership.acquire(context.noBackupFilesDir) } catch (error: Exception) { selectionProvider.close(); throw error }
    private val directory = File(context.noBackupFilesDir, "notebook")
    private val executor = BackendWriter()
    private val handle = CompletableFuture<Long>()
    private var closed = false
    private val nativeClosed = CompletableFuture<Void>()
    private val fullyClosed = CompletableFuture<Void>()
    private val reserve = File(directory, "emergency.reserve")
    private val pdf: PdfWorkspace
    private val selections: PdfSelectionManager
    private val outbox: CompletableFuture<CaptureOutbox>
    private val activeJournal: CompletableFuture<ActiveCaptureJournal>
    private val captureSessions = mutableSetOf<JournaledCaptureSession>()
    private var recoveryAttention = false
    init {
        var createdPdf: PdfWorkspace? = null
        var createdSelections: PdfSelectionManager? = null
        try {
            check(directory.isDirectory || directory.mkdirs())
            require(!java.nio.file.Files.isSymbolicLink(directory.toPath()))
            val loadedPdf = pdfFactory(context) { command ->
                JSONObject(request(JSONObject().put("version", 1).put("command", command).toString()).join())
            }
            createdPdf = loadedPdf
            pdf = loadedPdf
            val loadedSelections = PdfSelectionManager(selectionProvider,
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
                retentionDirectory = File(context.noBackupFilesDir, "pdf-retention-cache"),
                nativePdf = { command -> JSONObject(request(JSONObject().put("version", 1).put("command", JSONObject().put("op", "Pdf").put("command", command)).toString()).join()) },
            )
            createdSelections = loadedSelections
            selections = loadedSelections
            outbox = CompletableFuture.supplyAsync({ CaptureOutbox(File(directory, "capture-recovery")) }, executor)
            activeJournal = handle.thenApply { nativeHandle ->
                val journal = ActiveCaptureJournal(File(directory, "active-capture"), UUID.fromString(NativeBridge.captureLibraryIdentity(nativeHandle)))
                journal.recoverActive()
                try {
                    val legacy = outbox.join()
                    legacy.replay { request -> JSONObject(NativeBridge.request(nativeHandle, request)).getBoolean("ok") }
                    if (legacy.pendingCount() != 0) recoveryAttention = true
                } catch (_: Exception) { recoveryAttention = true }
                journal.drain { entry ->
                    try { JSONObject(NativeBridge.request(nativeHandle, CaptureRequest.encode(entry.ticket, entry.entity, entry.space, listOf(entry.stroke)))).getBoolean("ok") }
                    catch (_: Exception) { recoveryAttention = true; false }
                }
                recoveryAttention = recoveryAttention || journal.pendingCount() != 0 || journal.incompleteCount() != 0
                journal
            }
            // No native handle is opened until every synchronous factory succeeds.
            CompletableFuture.supplyAsync({ NativeBridge.open(File(directory, "library.db").absolutePath) }, executor).whenComplete { value, error ->
                if (error == null) handle.complete(value) else { handle.completeExceptionally(error); close() }
            }
        } catch (error: Throwable) {
            closed = true
            handle.completeExceptionally(error)
            try { createdPdf?.close() } catch (_: Exception) { }
            try { createdSelections?.close() } catch (_: Exception) { }
            try { selectionProvider.close() } catch (_: Exception) { }
            executor.shutdown()
            Thread({
                try {
                    awaitStopped(executor); createdPdf?.awaitClosed(); createdSelections?.awaitClosed(); selectionProvider.awaitClosed()
                    ownership.close()
                } catch (_: Exception) { /* ownership remains unavailable if cleanup fails */ }
            }, "stem-failed-open").apply { isDaemon = true; start() }
            throw error
        }
    }

    /** Observation cancellation does not cancel an admitted edit/checkpoint. */
    private fun <T> submitAdmitted(action: () -> T, writer: BackendWriter, authoring: Boolean = true, bytes: Long = 256): CompletableFuture<T> = writer.admit(authoring, bytes, action)
    private fun submitAdmittedVoid(action: () -> Unit, writer: BackendWriter, bytes: Long = 256): CompletableFuture<Void> {
        val result = CompletableFuture<Void>()
        writer.admit(true, bytes, action).whenComplete { _, error -> if (error == null) result.complete(null) else result.completeExceptionally(error) }
        return result
    }
    // Conservative input charges bound retained strings/vector frames, including
    // executing work. They are neither a whole-process RSS nor a latency promise.
    private fun inputBytes(vararg values: String): Long = 512L + values.sumOf { it.length.toLong() * 4 }
    private fun strokeBytes(strokes: List<CapturedStroke>, space: String): Long = inputBytes(space) + strokes.sumOf { 512L + inputBytes(it.id, it.session) + it.points.size.toLong() * 128 }
    fun writerStatus(): BackendAdmissionStatus = executor.snapshot()
    fun observeWriterStatus(): BackendStatusSubscription = executor.subscribe()
    @Synchronized fun openCaptureSession(entity: UUID, coordinateSpace: String): JournaledCaptureSession {
        check(!closed)
        val encoded = Charsets.UTF_8.newEncoder().onMalformedInput(java.nio.charset.CodingErrorAction.REPORT).onUnmappableCharacter(java.nio.charset.CodingErrorAction.REPORT).encode(java.nio.CharBuffer.wrap(coordinateSpace))
        require(coordinateSpace.isNotEmpty() && encoded.remaining() <= 256)
        check(captureSessions.size < 16)
        return JournaledCaptureSession(this, entity, coordinateSpace).also { captureSessions += it }
    }
    @Synchronized internal fun retireCaptureSession(session: JournaledCaptureSession) { captureSessions.remove(session) }
    @Synchronized internal fun checkpointCapture(session: JournaledCaptureSession, stroke: CapturedStroke, revision: Long, cancelled: Boolean): CompletableFuture<JournaledCaptureSession.Receipt> {
        check(!closed && session.backend === this)
        val snapshot = stroke.copy(points = stroke.points.toList())
        return submitAdmitted({ writeCheckpoint(session, snapshot, revision, cancelled) }, executor, bytes = strokeBytes(listOf(snapshot), session.space))
    }
    private fun nativeCaptureCommand(command: JSONObject): JSONObject {
        val result = JSONObject(NativeBridge.request(handle.join(), JSONObject().put("version", 1).put("command", command).toString()))
        if (!result.getBoolean("ok")) throw java.io.IOException("Capture recovery operation unavailable")
        return result.optJSONObject("data") ?: JSONObject()
    }
    private fun captureWasCommitted(ticket: UUID, entity: UUID): Boolean = nativeCaptureCommand(JSONObject().put("op", "CaptureReceipt").put("ticket", ticket.toString()).put("entity", entity.toString())).getBoolean("committed")
    private fun sendCheckpoint(entry: ActiveCaptureJournal.Entry): Boolean {
        val ok = try {
            val reply = JSONObject(NativeBridge.request(handle.join(), CaptureRequest.encode(entry.ticket, entry.entity, entry.space, listOf(entry.stroke))))
            reply.getInt("version") == 1 && reply.getBoolean("ok")
        } catch (_: Exception) { false }
        if (!ok) { recoveryAttention = true; executor.markAttention() }
        return ok
    }
    private fun writeCheckpoint(session: JournaledCaptureSession, stroke: CapturedStroke, revision: Long, cancelled: Boolean): JournaledCaptureSession.Receipt {
        require(UUID.fromString(stroke.session) == session.id && revision >= 0)
        val ticket = UUID.fromString(stroke.id); val journal = activeJournal.join()
        try {
            releaseReserveIfNeeded()
            if (!journal.contains(ticket) && ticket !in session.seen) {
                if (captureWasCommitted(ticket, session.entity)) {
                    require(!cancelled && stroke.end != StrokeEnd.InProgress)
                    val already = ActiveCaptureJournal.Entry(0, UUID.fromString(NativeBridge.captureLibraryIdentity(handle.join())), session.entity, session.space, revision, stroke)
                    check(sendCheckpoint(already))
                    return JournaledCaptureSession.Receipt(ticket, revision, true, true)
                }
                if (cancelled && stroke.sequence <= session.latestSequence) return JournaledCaptureSession.Receipt(ticket, revision, true, false, true)
                require(stroke.sequence > session.latestSequence)
                check(session.seen.size < 256)
                session.latestSequence = stroke.sequence
            }
            session.seen[ticket] = revision
            journal.checkpoint(session.entity, session.space, revision, stroke, cancelled)
            journal.drain(::sendCheckpoint)
            val remains = journal.contains(ticket)
            if (!remains) session.seen.remove(ticket)
            return JournaledCaptureSession.Receipt(ticket, revision, true, !cancelled && !remains, cancelled)
        } catch (error: Exception) { recoveryAttention = true; throw error }
    }
    data class CaptureRecoveryState(val pending: Int, val incomplete: Int, val needsAttention: Boolean)
    @Synchronized fun captureRecoveryState(): CompletableFuture<CaptureRecoveryState> {
        check(!closed)
        return submitAdmitted({ val journal = activeJournal.join(); CaptureRecoveryState(outbox.join().pendingCount() + journal.pendingCount(), journal.incompleteCount(), recoveryAttention || journal.incompleteCount() != 0 || journal.reviewCount() != 0) }, executor, authoring = false)
    }
    class CaptureRecoveryReview internal constructor(internal val owner: NotebookBackend, val ticket: UUID, val entity: UUID, val coordinateSpace: String, val stroke: CapturedStroke, internal val checksum: String)
    @Synchronized fun interruptedCaptureTickets(): CompletableFuture<List<UUID>> {
        check(!closed)
        return submitAdmitted({
            val journal = activeJournal.join()
            journal.reviewTickets().filter { ticket ->
                try { nativeCaptureCommand(JSONObject().put("op", "Read").put("id", journal.read(ticket).entity.toString())); true } catch (_: Exception) { false }
            }
        }, executor, authoring = false)
    }
    @Synchronized fun reviewInterruptedCapture(ticket: UUID): CompletableFuture<CaptureRecoveryReview> {
        check(!closed)
        return submitAdmitted({
            val journal = activeJournal.join(); val entry = journal.read(ticket); require(entry.review && !entry.cancelled)
            nativeCaptureCommand(JSONObject().put("op", "Read").put("id", entry.entity.toString()))
            CaptureRecoveryReview(this, ticket, entry.entity, entry.space, entry.stroke, journal.checksum(ticket))
        }, executor, authoring = false)
    }
    @Synchronized fun acceptInterruptedCapture(review: CaptureRecoveryReview, confirmed: Boolean): CompletableFuture<Int> {
        check(!closed && review.owner === this); require(confirmed)
        val ticket = review.ticket; val entity = review.entity; val checksum = review.checksum
        return submitAdmitted({
            val journal = activeJournal.join()
            nativeCaptureCommand(JSONObject().put("op", "Read").put("id", entity.toString()))
            journal.approve(ticket, checksum)
            replayCaptures()
        }, executor, bytes = inputBytes(checksum))
    }
    @Synchronized fun discardInterruptedCapture(review: CaptureRecoveryReview, confirmed: Boolean): CompletableFuture<Void> {
        check(!closed && review.owner === this); require(confirmed)
        val ticket = review.ticket; val entity = review.entity; val checksum = review.checksum
        return submitAdmittedVoid({
            nativeCaptureCommand(JSONObject().put("op", "Read").put("id", entity.toString()))
            activeJournal.join().discardReview(ticket, checksum)
        }, executor, bytes = inputBytes(checksum))
    }

    /** The caller chooses and retains this ticket before submission. Rejection
     * before admission leaves that caller's observations authoritative. After
     * admission, unsuccessful JNI replies remain in the durable recovery journal. */
    @Synchronized fun capture(ticket: UUID, entity: UUID, coordinateSpace: String, strokes: List<CompletedStroke>): CompletableFuture<Int> {
        check(!closed)
        require(strokes.size in 1..1000 && strokes.sumOf { it.points.size.toLong() } <= 100_000)
        // Snapshot mutable caller collections before crossing the thread boundary.
        val snapshots = strokes.map { it.copy(points = it.points.toList()) }
        return submitAdmitted({
            releaseReserveIfNeeded()
            outbox.join().enqueue(ticket, CaptureRequest.encode(ticket, entity, coordinateSpace, snapshots))
            replayCaptures()
        }, executor, bytes = strokeBytes(snapshots, coordinateSpace))
    }

    /** Call after opening/unlocking and after a recoverable storage failure. A
     * failed first capture stops replay without deleting it or later records. */
    @Synchronized fun retryCaptures(): CompletableFuture<Int> {
        check(!closed)
        return submitAdmitted({ releaseReserveIfNeeded(); replayCaptures() }, executor)
    }
    @Synchronized fun pendingCaptures(): CompletableFuture<Int> {
        check(!closed)
        return submitAdmitted({ outbox.join().pendingCount() + activeJournal.join().pendingCount() }, executor, authoring = false)
    }
    private fun replayCaptures(): Int {
        recoveryAttention = false
        val legacy = outbox.join().replay {
            val reply = JSONObject(NativeBridge.request(handle.join(), it))
            reply.getInt("version") == 1 && reply.getBoolean("ok")
        }
        val checkpoints = activeJournal.join().drain(::sendCheckpoint)
        if (outbox.join().pendingCount() != 0) recoveryAttention = true
        if (recoveryAttention) executor.markAttention()
        return legacy + checkpoints
    }
    private fun releaseReserveIfNeeded() {
        if (StatFs(directory.path).availableBytes < 8L * 1024 * 1024 && reserve.exists()) check(reserve.delete())
    }

    /** Completion with ok=true is the only commit acknowledgement. Queue rejection
     * leaves the caller responsible for retaining and retrying the same capture ticket. */
    @Synchronized fun request(json: String): CompletableFuture<String> {
        check(!closed)
        require(json.length <= 8 * 1024 * 1024)
        val command = JSONObject(json).getJSONObject("command")
        val authoring = !isQuery(command)
        if (affectsPdfPolicy(command)) { pdf.invalidate(); selections.invalidate() }
        return submitAdmitted({
            releaseReserveIfNeeded()
            val reply = NativeBridge.request(handle.join(), json)
            if (authoring && !JSONObject(reply).getBoolean("ok")) executor.markAttention()
            reply
        }, executor, authoring = authoring, bytes = inputBytes(json))
    }

    @Synchronized fun authenticate(scopesJson: String, proof: NativeBridge.Verification): CompletableFuture<Void> {
        check(!closed)
        require(scopesJson.length <= 65536)
        pdf.invalidate(); selections.invalidate()
        return submitAdmittedVoid({
            NativeBridge.authenticate(handle.join(), scopesJson, proof)
            try { replayCaptures() } catch (_: Exception) { recoveryAttention = true; executor.markAttention() }
        }, executor, bytes = inputBytes(scopesJson))
    }

    @Synchronized fun allocateReserve(bytes: Long = 4L * 1024 * 1024): CompletableFuture<Void> {
        check(!closed)
        return submitAdmittedVoid({
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
    @Synchronized fun retainPdf(source: UUID, decision: PdfSelectionManager.RetentionDecision, confirmed: Boolean, timeoutMillis: Long = 30_000): PdfSelectionManager.Pending<PdfSelectionManager.RetentionReceipt> {
        check(!closed)
        return selections.retain(source, decision, confirmed, timeoutMillis)
    }
    @Synchronized private fun prepareSelectionNative(target: String, locator: String, revision: String, proof: NativeBridge.PdfGrant): CompletableFuture<JSONObject> {
        check(!closed)
        return submitAdmitted({ JSONObject(NativeBridge.preparePdfSelection(handle.join(), target, locator, revision, proof)) }, executor, bytes = inputBytes(target, locator, revision))
    }
    @Synchronized private fun finishSelectionNative(review: String, confirmed: Boolean, proof: NativeBridge.PdfGrant): CompletableFuture<JSONObject> {
        check(!closed)
        pdf.invalidate()
        return submitAdmitted({ JSONObject(NativeBridge.finishPdfSelection(handle.join(), review, confirmed, proof)) }, executor, bytes = inputBytes(review))
    }
    /** Keep the native host's read classification; unknown/new commands retain
     * FIFO authoring semantics until deliberately classified. */
    private fun isQuery(command: JSONObject): Boolean {
        val op = command.getString("op")
        if (op == "Pdf") return command.getJSONObject("command").getString("op") in setOf(
            "Assets", "PrepareAssetRead", "ReadAssetChunk", "ValidateAssetRead", "Source", "Anchor", "PrepareRead", "ValidateRead", "ReviewRevision",
        )
        return op in setOf(
            "CaptureReceipt", "Block", "Mode", "OpenInMaster", "Layout", "Group", "Embed", "View", "RestoreView", "Links", "List", "Read", "Blocks", "Search", "Exercises", "ExerciseBlocks", "SourceInk", "History", "FlatExercises", "SearchViews", "Status",
        )
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
            "RestoreView", "CreateGroup", "Group", "TransformGroup", "Ungroup", "Embed", "CreateEmbed", "EditEmbed", "DuplicateEmbed", "RemoveEmbed", "View", "SaveView",
            "CaptureReceipt", "AddLink", "Links", "RemoveLink", "List", "Create", "Read", "Blocks", "Search", "Exercises", "ExerciseBlocks", "SourceInk", "History", "FlatExercises", "SearchViews", "Status",
            "CreateExercise", "RenameExercise", "ExerciseVisibility", "OpenInMaster", "Rename",
        )
    }

    /** Native-drain acknowledgement. Optional resources may still be retiring;
     * use closeFullyAsync before reopening this library. */
    fun closeAsync(): CompletableFuture<Void> { close(); return nativeClosed }
    /** Completes only after writer, provider/parser/cache work and ownership retire.
     * An uncooperative platform call keeps this pending; no new owner is admitted. */
    fun closeFullyAsync(): CompletableFuture<Void> { close(); return fullyClosed }

    override fun close() {
        val sessions = synchronized(this) {
            if (closed) return
            closed = true
            captureSessions.toList().also { captureSessions.clear() }
        }
        // Session input takes its own monitor before backend admission. Freeze
        // outside the backend monitor so close cannot invert that lock order.
        val closingCaptures = sessions.flatMap { session -> session.freezeForClose().map { session to it } }
        pdf.close(); selections.close()
        Thread({
            try {
                executor.shutdown()
                awaitStopped(executor)
                var checkpointFailure: Exception? = null
                if (!handle.isCompletedExceptionally) {
                    for ((session, capture) in closingCaptures) {
                        try { writeCheckpoint(session, capture.stroke, capture.revision, false) }
                        catch (error: Exception) { if (checkpointFailure == null) checkpointFailure = error }
                    }
                    NativeBridge.close(handle.join())
                }
                nativeClosed.complete(null)
                pdf.awaitClosed(); selections.awaitClosed()
                ownership.close()
                if (checkpointFailure == null) fullyClosed.complete(null) else fullyClosed.completeExceptionally(checkpointFailure)
            } catch (error: Exception) {
                nativeClosed.completeExceptionally(error); fullyClosed.completeExceptionally(error)
            }
        }, "stem-close").apply { isDaemon = true; start() }
    }
}
