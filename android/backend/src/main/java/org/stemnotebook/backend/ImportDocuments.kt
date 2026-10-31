package org.stemnotebook.backend

import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.CancellationSignal
import android.os.ParcelFileDescriptor
import android.os.Process
import java.io.IOException
import java.io.InputStream

/** Deliberate document-picker input; no persistent grant or automatic import. */
internal abstract class ImportDocuments {
    abstract fun validate(document: Uri)
    abstract fun open(document: Uri, signal: CancellationSignal): InputStream
    companion object {
        fun platform(context: Context): ImportDocuments = object : ImportDocuments() {
            private val platform = context.applicationContext
            override fun validate(document: Uri) {
                if (document.scheme != "content" || document.authority.isNullOrEmpty() || document.toString().length > 4096 ||
                    platform.checkUriPermission(document, Process.myPid(), Process.myUid(), Intent.FLAG_GRANT_READ_URI_PERMISSION) != PackageManager.PERMISSION_GRANTED)
                    throw IOException("Native import input unavailable")
            }
            override fun open(document: Uri, signal: CancellationSignal): InputStream {
                validate(document)
                val descriptor = platform.contentResolver.openFileDescriptor(document, "r", signal)
                    ?: throw IOException("Native import input unavailable")
                val stream = ParcelFileDescriptor.AutoCloseInputStream(descriptor)
                return object : java.io.FilterInputStream(stream) {
                    override fun close() { try { descriptor.checkError() } finally { super.close() } }
                }
            }
        }
    }
}
