package org.stemnotebook.backend

import java.util.UUID

/** One bounded exact-record snapshot. No raw observations are copied into it. */
internal data class DeletionCaptureStamp(val storage: DeletionCaptureStorage, val ticket: UUID, val entity: UUID, val checksum: String, val sealed: Boolean)
internal enum class DeletionCaptureStorage { ActiveJournal, LegacyOutbox }
data class DeletionCaptureDraft(val ticket: UUID, val entity: UUID, val sealed: Boolean)
enum class CaptureDeletionDisposition { KeepDrafts, DiscardReviewedSealed }
class DeletionCaptureReview internal constructor(
    internal val owner: NotebookBackend,
    internal val token: UUID,
    val entity: UUID,
    val permanent: Boolean,
    val hasDependencies: Boolean,
    dependants: List<UUID>,
    internal val targets: Set<UUID>,
    internal val stamps: List<DeletionCaptureStamp>,
    val unclassifiedRecovery: Int,
) {
    private val visible = dependants.toList()
    val visibleDependants: List<UUID> get() = visible.toList()
    val drafts: List<DeletionCaptureDraft> get() = stamps.map { DeletionCaptureDraft(it.ticket, it.entity, it.sealed) }
    internal var consumed = false // backend writer only
}
/** Document action is acknowledged separately from private draft cleanup. */
data class DeletionCaptureReceipt(val entity: UUID, val permanent: Boolean, val draftsRemoved: Int, val keptActive: Int, val keptSealed: Int, val cleanupUnconfirmed: Int)
