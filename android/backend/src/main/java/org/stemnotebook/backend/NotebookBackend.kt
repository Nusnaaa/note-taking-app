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
class NotebookBackend internal constructor(context: Context, pdfFactory: (Context, (JSONObject) -> JSONObject) -> PdfWorkspace, selectionProvider: PdfProvider = PdfProvider(context), captureJournalFault: (String) -> Unit = {}, backupFault: (String) -> Unit = {}, backupDocuments: BackupDocuments = BackupDocuments.platform(context), restoreFault: (String) -> Unit = {}, restoreDocuments: RestoreDocuments = RestoreDocuments.platform(context), existingLibrary: Boolean = false) : Closeable {
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
    private val backups: NotebookBackups
    private val restores: NotebookRestores
    private val backupEpoch = java.util.concurrent.atomic.AtomicReference(Any())
    private val outbox: CompletableFuture<CaptureOutbox>
    private val activeJournal: CompletableFuture<ActiveCaptureJournal>
    private val captureSessions = mutableSetOf<JournaledCaptureSession>()
    // Weak keys cover caller-held interrupted sessions without extending their
    // lifetime or retaining their observations after callers release them.
    private val captureSessionHistory = java.util.WeakHashMap<JournaledCaptureSession, Unit>()
    private var recoveryAttention = false
    private val capturePriority = mutableMapOf<UUID, Long>()
    @Volatile private var recognitionPriorityFailed = false
    init {
        var createdPdf: PdfWorkspace? = null
        var createdSelections: PdfSelectionManager? = null
        var createdBackups: NotebookBackups? = null
        var createdRestores: NotebookRestores? = null
        try {
            check(directory.isDirectory || directory.mkdirs())
            require(!java.nio.file.Files.isSymbolicLink(directory.toPath()))
            val loadedBackups = NotebookBackups(File(directory, "backup-staging"), context.noBackupFilesDir, { handle.join() },
                { expected -> backupBarrier(expected).join() },
                { expected -> try { backupBarrier(expected).join() === expected && backupStatusSnapshot().join().authoringQuiescent } catch (_: Exception) { false } }, backupFault, backupDocuments,
                { value -> (value as BackupStamp).let { BackupSnapshot(it.library, it.generation, true) } },
                { backupStatusSnapshot().join() })
            createdBackups = loadedBackups
            backups = loadedBackups
            val loadedRestores = NotebookRestores(File(directory, "restore-staging"), File(context.noBackupFilesDir, "recovered-libraries"), { handle.join() }, {
                val result = JSONObject(request(JSONObject().put("version", 1).put("command", JSONObject().put("op", "Status")).toString()).join())
                if (!result.getBoolean("ok")) throw NotebookRestores.Failure(NotebookRestores.FailureReason.AccessChanged)
            }, restoreDocuments, restoreFault)
            createdRestores = loadedRestores
            restores = loadedRestores
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
                val journal = ActiveCaptureJournal(File(directory, "active-capture"), UUID.fromString(NativeBridge.captureLibraryIdentity(nativeHandle)), fault = captureJournalFault)
                journal.recoverActive()
                try {
                    val legacy = outbox.join()
                    legacy.replay { request -> JSONObject(NativeBridge.request(nativeHandle, request)).getBoolean("ok") }
                    if (legacy.pendingCount() != 0) recoveryAttention = true
                } catch (_: Exception) { recoveryAttention = true }
                journal.drain { entry ->
                    try { JSONObject(NativeBridge.request(nativeHandle, CaptureRequest.encode(entry.ticket, entry.entity, entry.space, listOf(entry.stroke), entry.layer))).getBoolean("ok") }
                    catch (_: Exception) { recoveryAttention = true; false }
                }
                recoveryAttention = recoveryAttention || journal.pendingCount() != 0 || journal.incompleteCount() != 0
                journal
            }
            handle.thenAccept { _ -> synchronized(this) {
                if (!closed) capturePriority.keys.forEach { signalCapturePriority(it, true) }
            } }
            // No native handle is opened until every synchronous factory succeeds.
            CompletableFuture.supplyAsync({
                val path = File(directory, "library.db").absolutePath
                if (existingLibrary) NativeBridge.openExisting(path) else NativeBridge.open(path)
            }, executor).whenComplete { value, error ->
                if (error == null) handle.complete(value) else { handle.completeExceptionally(error); close() }
            }
        } catch (error: Throwable) {
            closed = true
            handle.completeExceptionally(error)
            try { createdRestores?.close() } catch (_: Exception) { }
            try { createdBackups?.close() } catch (_: Exception) { }
            try { createdPdf?.close() } catch (_: Exception) { }
            try { createdSelections?.close() } catch (_: Exception) { }
            try { selectionProvider.close() } catch (_: Exception) { }
            executor.shutdown()
            Thread({
                try {
                    awaitStopped(executor); createdRestores?.awaitClosed(); createdBackups?.awaitClosed(); createdPdf?.awaitClosed(); createdSelections?.awaitClosed(); selectionProvider.awaitClosed()
                    ownership.close()
                } catch (_: Exception) { /* ownership remains unavailable if cleanup fails */ }
            }, "stem-failed-open").apply { isDaemon = true; start() }
            throw error
        }
    }

    /** Observation cancellation does not cancel an admitted edit/checkpoint. */
    private fun <T> submitAdmitted(action: () -> T, writer: BackendWriter, authoring: Boolean = true, bytes: Long = 256): CompletableFuture<T> {
        if (authoring) backupEpoch.set(Any())
        return writer.admit(authoring, bytes, action)
    }
    private fun submitAdmittedVoid(action: () -> Unit, writer: BackendWriter, bytes: Long = 256): CompletableFuture<Void> {
        val result = CompletableFuture<Void>()
        submitAdmitted(action, writer, bytes = bytes).whenComplete { _, error -> if (error == null) result.complete(null) else result.completeExceptionally(error) }
        return result
    }
    // Conservative input charges bound retained strings/vector frames, including
    // executing work. They are neither a whole-process RSS nor a latency promise.
    private fun inputBytes(vararg values: String): Long = 512L + values.sumOf { it.length.toLong() * 4 }
    private fun strokeBytes(strokes: List<CapturedStroke>, space: String): Long = inputBytes(space) + strokes.sumOf { 512L + inputBytes(it.id, it.session) + it.points.size.toLong() * 128 }
    /** Trusted worker API. Publication and pending-draft checks are separate from save receipts. */
    @Synchronized fun backups(): NotebookBackups { check(!closed); return backups }
    @Synchronized fun restores(): NotebookRestores { check(!closed); return restores }
    private class BackupStamp(val epoch: Any, val generation: String, val library: String)
    @Synchronized private fun backupBarrier(expected: Any?): CompletableFuture<Any> {
        check(!closed)
        return submitAdmitted({
            activeJournal.join(); outbox.join()
            val epoch = backupEpoch.get()
            for (name in listOf("active-capture", "capture-recovery")) {
                val folder = File(directory, name)
                if (java.nio.file.Files.isSymbolicLink(folder.toPath()) || java.nio.file.Files.newDirectoryStream(folder.toPath()).use { it.iterator().hasNext() }) throw NotebookBackups.Failure(NotebookBackups.FailureReason.RecoveryRequired)
            }
            val generation = NativeBridge.backupGeneration(handle.join())
            val stamp = expected as BackupStamp?
            synchronized(this) {
                if (closed || capturePriority.isNotEmpty() || recognitionPriorityFailed || backupEpoch.get() !== epoch || (stamp != null && (stamp.epoch !== epoch || stamp.generation != generation))) throw NotebookBackups.Failure(NotebookBackups.FailureReason.SourceChanged)
            }
            stamp ?: BackupStamp(epoch, generation, NativeBridge.captureLibraryIdentity(handle.join()))
        }, executor, authoring = false)
    }
    @Synchronized private fun backupStatusSnapshot(): CompletableFuture<BackupSnapshot> {
        check(!closed)
        return submitAdmitted({
            activeJournal.join(); outbox.join()
            val epoch = backupEpoch.get()
            // Native generation checks current app authority before private history
            // can be read. Pending drafts affect coverage, never local Saved state.
            val native = handle.join()
            val generation = NativeBridge.backupGeneration(native)
            val library = NativeBridge.captureLibraryIdentity(native)
            var quiet = true
            for (name in listOf("active-capture", "capture-recovery")) {
                val folder = File(directory, name)
                if (java.nio.file.Files.isSymbolicLink(folder.toPath()) || java.nio.file.Files.newDirectoryStream(folder.toPath()).use { it.iterator().hasNext() }) quiet = false
            }
            synchronized(this) {
                check(!closed)
                val admission = executor.snapshot()
                BackupSnapshot(library, generation, quiet && !recoveryAttention && !admission.needsAttention && admission.pendingAuthoring == 0 && capturePriority.isEmpty() && !recognitionPriorityFailed && backupEpoch.get() === epoch)
            }
        }, executor, authoring = false)
    }
    data class RecognitionStatus(val reserved: Int, val running: Int, val closed: Boolean, val captureActive: Boolean, val prioritySignalFailed: Boolean)
    fun recognitionStatus(): CompletableFuture<RecognitionStatus> = handle.thenApply { native ->
        val status = JSONObject(NativeBridge.recognitionStatus(native))
        RecognitionStatus(status.getInt("reserved"), status.getInt("running"), status.getBoolean("closed"), status.getBoolean("capture_active"), recognitionPriorityFailed)
    }
    // A newer observed frame owns the release. An old pen-up completion cannot
    // resume optional work during a subsequent contact in the same session.
    @Synchronized internal fun beginCapturePriority(session: JournaledCaptureSession, revision: Long) {
        if (closed) return
        backupEpoch.set(Any())
        capturePriority[session.id] = revision
        signalCapturePriority(session.id, true)
    }
    private fun signalCapturePriority(session: UUID, active: Boolean) {
        try {
            val native = handle.getNow(0L)
            if (native != 0L) NativeBridge.setCapturePriority(native, session.toString(), active)
        } catch (_: Exception) { recognitionPriorityFailed = true }
    }
    internal fun releaseCapturePriorityAfter(session: JournaledCaptureSession, revision: Long, receipts: List<CompletableFuture<JournaledCaptureSession.Receipt>>) {
        CompletableFuture.allOf(*receipts.toTypedArray()).whenComplete { _, _ -> synchronized(this) {
            if (capturePriority[session.id] == revision) {
                capturePriority.remove(session.id)
                if (!closed) signalCapturePriority(session.id, false)
            }
        } }
    }
    fun writerStatus(): BackendAdmissionStatus = executor.snapshot()
    fun observeWriterStatus(): BackendStatusSubscription = executor.subscribe()
    @Synchronized fun openCaptureSession(entity: UUID, coordinateSpace: String, style: StrokeStyle? = null, layer: CaptureLayer? = null, fingerInk: Boolean = false): JournaledCaptureSession {
        check(!closed)
        val encoded = Charsets.UTF_8.newEncoder().onMalformedInput(java.nio.charset.CodingErrorAction.REPORT).onUnmappableCharacter(java.nio.charset.CodingErrorAction.REPORT).encode(java.nio.CharBuffer.wrap(coordinateSpace))
        require(coordinateSpace.isNotEmpty() && encoded.remaining() <= 256)
        check(captureSessions.size < 16)
        return JournaledCaptureSession(this, entity, coordinateSpace, style, layer, fingerInk).also { captureSessions += it; captureSessionHistory[it] = Unit }
    }
    @Synchronized internal fun retireCaptureSession(session: JournaledCaptureSession) { captureSessions.remove(session) }
    // Writer-only seen state; removing this exact ticket restores the existing
    // begin-order rejection for old frames. Other pending tickets remain retryable.
    private fun retireDiscardedFrame(entity: UUID, ticket: UUID) {
        val sessions = synchronized(this) { captureSessionHistory.keys.toList() }
        sessions.filter { it.entity == entity }.forEach { it.seen.remove(ticket) }
    }
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
            val reply = JSONObject(NativeBridge.request(handle.join(), CaptureRequest.encode(entry.ticket, entry.entity, entry.space, listOf(entry.stroke), entry.layer)))
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
            // Replay/review can retire a file while the caller still owns an old
            // session frame. Probe SQLite before recreating any missing record.
            if (!journal.contains(ticket)) {
                if (captureWasCommitted(ticket, session.entity)) {
                    require(!cancelled && stroke.end != StrokeEnd.InProgress)
                    val already = ActiveCaptureJournal.Entry(0, UUID.fromString(NativeBridge.captureLibraryIdentity(handle.join())), session.entity, session.space, revision, stroke, layer = session.layer)
                    check(sendCheckpoint(already))
                    return JournaledCaptureSession.Receipt(ticket, revision, true, true)
                }
                if (ticket !in session.seen) {
                    if (cancelled && stroke.sequence <= session.latestSequence) {
                        return JournaledCaptureSession.Receipt(ticket, revision, true, false, true)
                    }
                    require(stroke.sequence > session.latestSequence)
                    check(session.seen.size < 256)
                    session.latestSequence = stroke.sequence
                }
            }
            session.seen[ticket] = revision
            journal.checkpoint(session.entity, session.space, revision, stroke, cancelled, session.layer)
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
    enum class CaptureTargetState { Active, Trashed, Purged }
    class CaptureRecoveryReview internal constructor(internal val owner: NotebookBackend, val ticket: UUID, val entity: UUID, val coordinateSpace: String, val stroke: CapturedStroke, val targetState: CaptureTargetState, val layer: CaptureLayer?, val layerAvailable: Boolean?, internal val checksum: String)
    private fun recoveryAccess(entity: UUID): CaptureTargetState = CaptureTargetState.valueOf(nativeCaptureCommand(JSONObject().put("op", "CaptureRecoveryAccess").put("entity", entity.toString())).getString("state"))
    private fun recoveryLayerAvailable(entry: ActiveCaptureJournal.Entry, state: CaptureTargetState): Boolean? {
        if (state != CaptureTargetState.Active) return false
        val layer = entry.layer ?: return true
        return try {
            val layers = nativeCaptureCommand(JSONObject().put("op", "CanvasLayers").put("canvas", entry.entity.toString())).getJSONArray("layers")
            layer == CaptureLayer.Base || (0 until layers.length()).any { layers.getJSONObject(it).getString("id") == (layer as CaptureLayer.Custom).id.toString() }
        } catch (_: java.io.IOException) { null } // Unknown metadata never blocks review of independently authored evidence.
    }
    @Synchronized fun interruptedCaptureTickets(): CompletableFuture<List<UUID>> {
        check(!closed)
        return submitAdmitted({
            val journal = activeJournal.join()
            journal.reviewTickets().filter { ticket ->
                try { recoveryAccess(journal.read(ticket).entity); true } catch (_: Exception) { false }
            }
        }, executor, authoring = false)
    }
    @Synchronized fun reviewInterruptedCapture(ticket: UUID): CompletableFuture<CaptureRecoveryReview> {
        check(!closed)
        return submitAdmitted({
            val journal = activeJournal.join(); val entry = journal.read(ticket); require(entry.stroke.end != StrokeEnd.InProgress && !entry.cancelled)
            val state = recoveryAccess(entry.entity)
            CaptureRecoveryReview(this, ticket, entry.entity, entry.space, entry.stroke, state, entry.layer, recoveryLayerAvailable(entry, state), journal.checksum(ticket))
        }, executor, authoring = false)
    }
    @Synchronized fun acceptInterruptedCapture(review: CaptureRecoveryReview, confirmed: Boolean): CompletableFuture<Int> {
        check(!closed && review.owner === this); require(confirmed)
        val ticket = review.ticket; val entity = review.entity; val checksum = review.checksum
        return submitAdmitted({
            val journal = activeJournal.join()
            nativeCaptureCommand(JSONObject().put("op", "Read").put("id", entity.toString()))
            val entry = journal.read(ticket)
            require(recoveryLayerAvailable(entry, CaptureTargetState.Active) == true) { "Capture layer requires review" }
            journal.approve(ticket, checksum)
            replayCaptures()
        }, executor, bytes = inputBytes(checksum))
    }
    class CaptureRetargetReview internal constructor(internal val owner: NotebookBackend, val source: CaptureRecoveryReview, val destination: CaptureLayer, val layerRevision: Long)
    data class CaptureRetargetReceipt(val ticket: UUID, val destination: CaptureLayer, val checkpointed: Boolean, val committed: Boolean)
    private fun retargetLayers(review: CaptureRecoveryReview, destination: CaptureLayer): JSONObject {
        require(recoveryAccess(review.entity) == CaptureTargetState.Active)
        val journal = activeJournal.join()
        require(journal.checksum(review.ticket) == review.checksum)
        val entry = journal.read(review.ticket)
        require(entry.entity == review.entity && entry.stroke.end != StrokeEnd.InProgress && !entry.cancelled && entry.layer != destination)
        // Never reinterpret an already published capture or an ambiguous receipt.
        require(!captureWasCommitted(entry.ticket, entry.entity)) { "Capture already committed" }
        require(!outbox.join().containsTicket(entry.ticket)) { "Conflicting legacy capture" }
        val state = nativeCaptureCommand(JSONObject().put("op", "CanvasLayers").put("canvas", entry.entity.toString()))
        val layers = state.getJSONArray("layers")
        require(destination == CaptureLayer.Base || (0 until layers.length()).any { layers.getJSONObject(it).getString("id") == (destination as CaptureLayer.Custom).id.toString() }) { "Capture destination unavailable" }
        return state
    }
    /** Explicit same-canvas routing review; no fallback, source copy or publication. */
    @Synchronized fun reviewCaptureRetarget(review: CaptureRecoveryReview, destination: CaptureLayer): CompletableFuture<CaptureRetargetReview> {
        check(!closed && review.owner === this)
        return submitAdmitted({ CaptureRetargetReview(this, review, destination, retargetLayers(review, destination).getLong("revision")) }, executor, authoring = false, bytes = inputBytes(review.checksum))
    }
    @Synchronized fun confirmCaptureRetarget(review: CaptureRetargetReview, confirmed: Boolean): CompletableFuture<CaptureRetargetReceipt> {
        check(!closed && review.owner === this && review.source.owner === this); require(confirmed)
        return submitAdmitted({
            require(retargetLayers(review.source, review.destination).getLong("revision") == review.layerRevision) { "Capture destination changed" }
            val journal = activeJournal.join(); val ticket = review.source.ticket
            releaseReserveIfNeeded()
            journal.retarget(ticket, review.source.checksum, review.destination)
            journal.approve(ticket, journal.checksum(ticket))
            replayCaptures()
            CaptureRetargetReceipt(ticket, review.destination, true, !journal.contains(ticket))
        }, executor, bytes = inputBytes(review.source.checksum))
    }
    @Synchronized fun discardInterruptedCapture(review: CaptureRecoveryReview, confirmed: Boolean): CompletableFuture<Void> {
        check(!closed && review.owner === this); require(confirmed)
        val ticket = review.ticket; val entity = review.entity; val checksum = review.checksum
        return submitAdmittedVoid({
            recoveryAccess(entity)
            activeJournal.join().discardReview(ticket, checksum) { retireDiscardedFrame(entity, ticket) }
        }, executor, bytes = inputBytes(checksum))
    }

    private fun deletionStamps(targets: Set<UUID>): Pair<List<DeletionCaptureStamp>, Int> {
        val journal = activeJournal.join(); val legacy = outbox.join().deletionSnapshot(targets)
        return (journal.deletionSnapshot(targets) + legacy.stamps) to (journal.incompleteCount() + legacy.unclassified)
    }
    private fun releaseDeletionToken(token: UUID) {
        nativeCaptureCommand(JSONObject().put("op", "ReleaseDeletion").put("review", token.toString()))
    }
    /** Review native dependencies and exact private recovery records without publishing ink. */
    @Synchronized fun prepareDeletionWithCaptures(entity: UUID, permanent: Boolean): CompletableFuture<DeletionCaptureReview> {
        check(!closed)
        return submitAdmitted({
            val data = nativeCaptureCommand(JSONObject().put("op", "PrepareDeletion").put("entity", entity.toString()).put("permanent", permanent))
            val token = UUID.fromString(data.getString("review"))
            try {
                val targetsJson = data.getJSONArray("capture_targets")
                val targets = (0 until targetsJson.length()).map { UUID.fromString(targetsJson.getString(it)) }.toSet()
                val (stamps, unknown) = deletionStamps(targets)
                stamps.map { it.entity }.distinct().forEach { recoveryAccess(it) }
                val visible = data.getJSONArray("visible_dependants")
                DeletionCaptureReview(this, token, entity, permanent, data.getBoolean("has_dependencies"), (0 until visible.length()).map { UUID.fromString(visible.getString(it)) }, targets, stamps, unknown)
            } catch (error: Exception) {
                try { releaseDeletionToken(token) } catch (_: Exception) { /* bounded native review expires or retires on close */ }
                throw error
            }
        }, executor, bytes = inputBytes(entity.toString()))
    }
    @Synchronized fun releaseDeletionWithCaptures(review: DeletionCaptureReview): CompletableFuture<Void> {
        check(!closed && review.owner === this)
        return submitAdmittedVoid({
            if (!review.consumed) { review.consumed = true; releaseDeletionToken(review.token) }
        }, executor)
    }
    /** Confirmation is one admitted operation. Cancellation cancels observation only.
     * Native deletion commits first; only its successful acknowledgement permits
     * disposal of the exact reviewed sealed recovery copies. Active/unknown stay. */
    @Synchronized fun confirmDeletionWithCaptures(review: DeletionCaptureReview, disposition: CaptureDeletionDisposition, confirmed: Boolean): CompletableFuture<DeletionCaptureReceipt> {
        check(!closed && review.owner === this); require(confirmed)
        backups.invalidate(); restores.invalidate(); pdf.invalidate(); selections.invalidate()
        return submitAdmitted({
            check(!review.consumed); review.consumed = true
            try {
                nativeCaptureCommand(JSONObject().put("op", "ValidateDeletionReview").put("review", review.token.toString()))
                val (current, _) = deletionStamps(review.targets)
                current.map { it.entity }.distinct().forEach { recoveryAccess(it) }
                require(current == review.stamps) { "Capture drafts changed; review again" }
                nativeCaptureCommand(JSONObject().put("op", "ConfirmDeletion").put("review", review.token.toString()).put("confirmed", true))
            } catch (error: Exception) {
                try { releaseDeletionToken(review.token) } catch (_: Exception) { }
                throw error // no recovery copy is removed on failed/ambiguous native acknowledgement
            }
            var removed = 0; var unconfirmed = 0
            val sealed = review.stamps.filter { it.sealed }
            if (disposition == CaptureDeletionDisposition.DiscardReviewedSealed) for (stamp in sealed) {
                try {
                    when (stamp.storage) {
                        DeletionCaptureStorage.ActiveJournal -> activeJournal.join().discardReview(stamp.ticket, stamp.checksum) { retireDiscardedFrame(stamp.entity, stamp.ticket) }
                        DeletionCaptureStorage.LegacyOutbox -> outbox.join().discardForDeletion(stamp)
                    }
                    removed++
                } catch (_: Exception) { unconfirmed++; recoveryAttention = true; executor.markAttention() }
            }
            DeletionCaptureReceipt(review.entity, review.permanent, removed, review.stamps.count { !it.sealed }, if (disposition == CaptureDeletionDisposition.KeepDrafts) sealed.size else 0, unconfirmed)
        }, executor, bytes = 512L + review.stamps.size.toLong() * 256)
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
        if (affectsPdfPolicy(command)) { backups.invalidate(); restores.invalidate(); pdf.invalidate(); selections.invalidate() }
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
        backups.invalidate(); restores.invalidate(); pdf.invalidate(); selections.invalidate()
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
            "Proposals", "Proposal", "ViewPen", "ViewGuides", "TrashEntries", "CaptureRecoveryAccess", "CanvasLayers", "CanvasScene", "CaptureReceipt", "Block", "Mode", "OpenInMaster", "Layout", "Group", "Embed", "Views", "View", "RestoreView", "RestoreWorkspace", "Links", "List", "Read", "Blocks", "Search", "Exercises", "ExerciseBlocks", "SourceInk", "History", "FlatExercises", "SearchViews", "Status", "Help",
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
            "RestoreView", "RestoreWorkspace", "SaveWorkspace", "CreateGroup", "Group", "TransformGroup", "Ungroup", "Embed", "CreateEmbed", "EditEmbed", "DuplicateEmbed", "RemoveEmbed", "Views", "View", "SaveView",
            "Proposals", "Proposal", "CorrectProposal", "RejectProposal", "AcceptProposal", "AcceptProposals", "ConfigureView", "ApplyViewPreset", "ViewPen", "ViewGuides", "TrashEntries", "PrepareDeletion", "ValidateDeletionReview", "ReleaseDeletion", "CaptureRecoveryAccess", "CaptureReceipt", "AddLink", "Links", "RemoveLink", "List", "Create", "Read", "Blocks", "Search", "Exercises", "ExerciseBlocks", "SourceInk", "History", "FlatExercises", "SearchViews", "Status", "Help",
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
        backups.close(); restores.close(); pdf.close(); selections.close()
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
                backups.awaitClosed(); restores.awaitClosed(); pdf.awaitClosed(); selections.awaitClosed()
                ownership.close()
                if (checkpointFailure == null) fullyClosed.complete(null) else fullyClosed.completeExceptionally(checkpointFailure)
            } catch (error: Exception) {
                nativeClosed.completeExceptionally(error); fullyClosed.completeExceptionally(error)
            }
        }, "stem-close").apply { isDaemon = true; start() }
    }
}
