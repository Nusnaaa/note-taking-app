package org.stemnotebook.backend

import java.util.UUID
import org.json.JSONObject

/** Fixed destination for one capture session, separate from observed stroke data.
 * Change destination by interrupting the old session and opening a new one. */
sealed class CaptureLayer {
    data object Base : CaptureLayer()
    data class Custom(val id: UUID) : CaptureLayer()
    internal fun wire(): Any = when (this) {
        Base -> "Base"
        is Custom -> JSONObject().put("Custom", id.toString())
    }
}
