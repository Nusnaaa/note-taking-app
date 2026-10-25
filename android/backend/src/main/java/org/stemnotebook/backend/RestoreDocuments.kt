package org.stemnotebook.backend

import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.CancellationSignal
import android.os.ParcelFileDescriptor
import android.os.Process
import java.io.InputStream
import java.io.IOException

/** Deliberate picker input. No persistent grant is acquired automatically. */
internal abstract class RestoreDocuments {
    abstract fun validate(document: Uri)
    abstract fun open(document: Uri, signal: CancellationSignal): InputStream
    companion object {
        fun platform(context: Context): RestoreDocuments = object : RestoreDocuments() {
            private val platform = context.applicationContext
            override fun validate(document: Uri) {
                if (platform.checkUriPermission(document, Process.myPid(), Process.myUid(), Intent.FLAG_GRANT_READ_URI_PERMISSION) != PackageManager.PERMISSION_GRANTED)
                    throw IOException("Restore input unavailable")
            }
            override fun open(document: Uri, signal: CancellationSignal): InputStream {
                validate(document)
                val descriptor = platform.contentResolver.openFileDescriptor(document, "r", signal)
                    ?: throw IOException("Restore input unavailable")
                val stream = ParcelFileDescriptor.AutoCloseInputStream(descriptor)
                return object : java.io.FilterInputStream(stream) {
                    override fun close() { try { descriptor.checkError() } finally { super.close() } }
                }
            }
        }
    }
}
