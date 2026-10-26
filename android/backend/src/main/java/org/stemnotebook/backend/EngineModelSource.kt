package org.stemnotebook.backend

import android.content.ContentResolver
import android.content.Context
import android.content.Intent
import android.net.Uri
import java.io.InputStream

/** Explicit picker observation for read-only local model input. It neither
 * persists a URI grant nor downloads code; resume may require selecting again. */
class EngineModelSource private constructor(private val resolver: ContentResolver, private val uri: Uri) {
    companion object {
        fun fromPicker(context: Context, result: Intent): EngineModelSource {
            val uri = result.data ?: throw NotebookEngines.Failure(NotebookEngines.FailureReason.InvalidInput)
            if (uri.scheme != "content" || uri.toString().length > 4096 ||
                result.flags and Intent.FLAG_GRANT_READ_URI_PERMISSION == 0) throw NotebookEngines.Failure(NotebookEngines.FailureReason.InvalidInput)
            return EngineModelSource(context.applicationContext.contentResolver,uri)
        }
    }
    /** Called inside admitted native running work, on the optional worker. A
     * blocking provider remains owned until it actually returns/closes. */
    internal fun open(offset: Long, control: NativeBridge.EngineControl): NativeBridge.EngineInput {
        control.check()
        val descriptor = resolver.openAssetFileDescriptor(uri,"r") ?: throw NotebookEngines.Failure(NotebookEngines.FailureReason.Unavailable)
        var input: InputStream? = null
        try {
            val stream = descriptor.createInputStream(); input = stream
            var remaining = offset
            val scratch = ByteArray(65536)
            try {
                while (remaining > 0) {
                    control.check()
                    val count = stream.read(scratch,0,minOf(remaining,scratch.size.toLong()).toInt())
                    if (count <= 0) throw NotebookEngines.Failure(NotebookEngines.FailureReason.Unavailable)
                    remaining -= count
                }
            } finally { scratch.fill(0) }
            control.check()
            return object : NativeBridge.EngineInput {
                override fun read(buffer: ByteArray, current: NativeBridge.EngineControl): Int {
                    current.check(); val count = stream.read(buffer); current.check(); return count
                }
                override fun retire() {
                    var failure: Exception? = null
                    try { stream.close() } catch (error: Exception) {failure=error}
                    try { descriptor.close() } catch (error: Exception) {failure=error}
                    if (failure != null) throw NotebookEngines.Failure(NotebookEngines.FailureReason.Unavailable)
                }
            }
        } catch (error: Throwable) {
            try {input?.close()} catch (_: Throwable) {}
            try {descriptor.close()} catch (_: Throwable) {}
            throw NotebookEngines.Failure(NotebookEngines.FailureReason.Unavailable)
        }
    }
}
