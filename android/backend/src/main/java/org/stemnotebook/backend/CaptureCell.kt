package org.stemnotebook.backend

import java.util.UUID

/** Immutable table source binding. Revision advances only after an exact native
 * commit acknowledgement, or an explicit reviewed recovery routing decision. */
data class CaptureCell(val block: UUID, val cell: UUID, val revision: Long) {
    init { require(revision >= 0) }
}
