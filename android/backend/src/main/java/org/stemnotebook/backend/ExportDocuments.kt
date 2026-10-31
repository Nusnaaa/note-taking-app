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

/** Deliberate SAF tree capability. Never acquires a persistent grant. */
internal abstract class ExportDocuments {
    data class Selection(val root: Uri, val identity: String, val label: String)
    abstract fun inspect(tree: Uri, signal: CancellationSignal): Selection
    abstract fun create(parent: Uri, name: String, mime: String): Uri
    abstract fun isChild(parent: Uri, document: Uri): Boolean
    abstract fun name(document: Uri, signal: CancellationSignal): String
    abstract fun open(document: Uri, write: Boolean, signal: CancellationSignal): ParcelFileDescriptor
    companion object {
        fun platform(context: Context): ExportDocuments = object : ExportDocuments() {
            private val platform = context.applicationContext
            private val resolver = platform.contentResolver
            override fun inspect(tree: Uri, signal: CancellationSignal): Selection {
                val access = Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                if (!DocumentsContract.isTreeUri(tree) || platform.checkUriPermission(tree,Process.myPid(),Process.myUid(),access) != PackageManager.PERMISSION_GRANTED)
                    throw IOException("Export destination unavailable")
                val id = DocumentsContract.getTreeDocumentId(tree)
                val root = DocumentsContract.buildDocumentUriUsingTree(tree,id)
                resolver.query(root,arrayOf(DocumentsContract.Document.COLUMN_DOCUMENT_ID,DocumentsContract.Document.COLUMN_MIME_TYPE,
                    DocumentsContract.Document.COLUMN_FLAGS,DocumentsContract.Document.COLUMN_DISPLAY_NAME),null,null,null,signal).use { cursor ->
                    check(cursor != null && cursor.moveToFirst())
                    check(cursor.getString(0)==id && cursor.getString(1)==DocumentsContract.Document.MIME_TYPE_DIR)
                    check(cursor.getLong(2) and DocumentsContract.Document.FLAG_DIR_SUPPORTS_CREATE.toLong()!=0L)
                    val label = cursor.getString(3) ?: ""; require(id.length<=4096 && label.length<=4096)
                    return Selection(root,id,label)
                }
            }
            override fun create(parent: Uri,name: String,mime: String): Uri =
                DocumentsContract.createDocument(resolver,parent,mime,name) ?: throw IOException("Export creation uncertain")
            override fun isChild(parent: Uri,document: Uri): Boolean = DocumentsContract.isChildDocument(resolver,parent,document)
            override fun name(document: Uri,signal: CancellationSignal): String =
                resolver.query(document,arrayOf(DocumentsContract.Document.COLUMN_DISPLAY_NAME),null,null,null,signal).use { c ->
                    check(c!=null && c.moveToFirst());requireNotNull(c.getString(0))
                }
            override fun open(document: Uri,write: Boolean,signal: CancellationSignal): ParcelFileDescriptor =
                resolver.openFileDescriptor(document,if(write) "wa" else "r",signal) ?: throw IOException("Export document unavailable")
        }
    }
}
