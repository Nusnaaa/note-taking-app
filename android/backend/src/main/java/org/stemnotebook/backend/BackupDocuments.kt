package org.stemnotebook.backend

import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.CancellationSignal
import android.os.ParcelFileDescriptor
import android.os.Process
import android.provider.DocumentsContract
import java.io.IOException

/** Trusted provider adapter. Selection comes from a deliberate tree picker, never
 * notebook/model JSON. It owns no persistent grants, threads or descriptor clones. */
internal abstract class BackupDocuments {
    data class Selection(val identity: String, val label: String)
    abstract fun inspect(tree: Uri, signal: CancellationSignal): Selection
    abstract fun create(tree: Uri, name: String): Uri
    abstract fun isChild(tree: Uri, document: Uri): Boolean
    abstract fun open(document: Uri, write: Boolean, signal: CancellationSignal): ParcelFileDescriptor
    companion object {
        fun platform(context: Context): BackupDocuments = object : BackupDocuments() {
            private val platform = context.applicationContext
            private val resolver = platform.contentResolver
            private val access = Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
            override fun inspect(tree: Uri, signal: CancellationSignal): Selection {
                if (platform.checkUriPermission(tree, Process.myPid(), Process.myUid(), access) != PackageManager.PERMISSION_GRANTED)
                    throw SecurityException("Backup destination access unavailable")
                val id = DocumentsContract.getTreeDocumentId(tree)
                val root = DocumentsContract.buildDocumentUriUsingTree(tree, id)
                val fields = arrayOf(DocumentsContract.Document.COLUMN_DOCUMENT_ID, DocumentsContract.Document.COLUMN_MIME_TYPE,
                    DocumentsContract.Document.COLUMN_FLAGS, DocumentsContract.Document.COLUMN_DISPLAY_NAME)
                val cursor = resolver.query(root, fields, null, null, null, signal) ?: throw IOException("Backup directory unavailable")
                cursor.use {
                    check(it.moveToFirst())
                    check(it.getString(0) == id && it.getString(1) == DocumentsContract.Document.MIME_TYPE_DIR)
                    check(it.getLong(2) and DocumentsContract.Document.FLAG_DIR_SUPPORTS_CREATE.toLong() != 0L)
                    val label = it.getString(3) ?: ""
                    require(id.length <= 4096 && label.length <= 4096)
                    return Selection(id, label)
                }
            }
            override fun create(tree: Uri, name: String): Uri = DocumentsContract.createDocument(resolver,
                DocumentsContract.buildDocumentUriUsingTree(tree, DocumentsContract.getTreeDocumentId(tree)),
                "application/octet-stream", name) ?: throw IOException("Backup document creation uncertain")
            override fun isChild(tree: Uri, document: Uri): Boolean = DocumentsContract.isChildDocument(resolver,
                DocumentsContract.buildDocumentUriUsingTree(tree, DocumentsContract.getTreeDocumentId(tree)), document)
            override fun open(document: Uri, write: Boolean, signal: CancellationSignal): ParcelFileDescriptor =
                resolver.openFileDescriptor(document, if (write) "w" else "r", signal) ?: throw IOException("Backup document unavailable")
        }
    }
}
