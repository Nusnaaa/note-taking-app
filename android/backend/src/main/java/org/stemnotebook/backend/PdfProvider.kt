package org.stemnotebook.backend

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.ParcelFileDescriptor

/** Link-first: opening obtains no managed copy and never grants export authority. */
class PdfProvider(private val context: Context) {
    fun retainReadGrant(uri: Uri, pickerFlags: Int) {
        require(uri.scheme == "content")
        require(pickerFlags and Intent.FLAG_GRANT_READ_URI_PERMISSION != 0)
        context.contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
    }
    fun openReadOnly(uri: Uri): ParcelFileDescriptor {
        require(uri.scheme == "content")
        return context.contentResolver.openFileDescriptor(uri, "r") ?: error("Source unavailable")
    }
    fun revoke(uri: Uri) { context.contentResolver.releasePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION) }
}
