package org.stemnotebook.backend

import java.util.UUID
import org.json.JSONObject

/** Fixed destination for one capture session, separate from observed stroke data.
 * Change future contacts by interrupting the old session and opening a new one.
 * Sealed uncommitted evidence has a separate explicit reviewed-retarget workflow. */
sealed class CaptureLayer {
    data object Base : CaptureLayer()
    data class Custom(val id: UUID) : CaptureLayer()
    internal fun wire(): Any = when (this) {
        Base -> "Base"
        is Custom -> JSONObject().put("Custom", id.toString())
    }
}
