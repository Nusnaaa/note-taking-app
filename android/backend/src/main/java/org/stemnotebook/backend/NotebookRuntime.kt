package org.stemnotebook.backend

import android.content.Context
import android.content.ContextWrapper
import android.os.Looper
import java.io.Closeable
import java.io.File

/** Process-local shared backend entry point for foreground and scheduled work.
 * Acquire/await/canonical-path checks belong on a worker. No job is enabled here.
 * Legacy NotebookBackend constructors remain exclusive and cannot be adopted. */
object NotebookRuntime {
    internal class Root(val path: String, val application: Context) {
        override fun equals(other: Any?): Boolean = other is Root && path == other.path
        override fun hashCode(): Int = path.hashCode()
    }
    private val pool = SharedNotebookPool<Root, NotebookBackend>(
        { root, create ->
            val context = object : ContextWrapper(root.application) {
                override fun getNoBackupFilesDir(): File = File(root.path)
            }
            NotebookBackend(context, pdfFactory = { platform, native -> PdfWorkspace(platform, native) }, existingLibrary = !create)
        },
        { it.runtimeReady() }, { it.runtimeClosing() }, { it.runtimeOwnershipReleased() }, { it.closeFullyAsync() },
    )
    class Pending internal constructor(private val request: SharedNotebookPool<Root, NotebookBackend>.Pending) : Closeable {
        fun await(): Lease { workerOnly(); return Lease(request.await()) }
        fun cancel(): Boolean = request.cancel()
        override fun close() { request.close() }
    }
    class Lease internal constructor(private val lease: SharedNotebookPool<Root, NotebookBackend>.Lease) : Closeable {
        val backend: NotebookBackend get() = lease.value()
        /** Release only after this consumer's operations have actually retired. */
        override fun close() { lease.close() }
        /** Worker-only observation. Other live leases keep the backend open. */
        fun awaitClosed() { workerOnly(); lease.awaitClosed() }
    }
    /** Existing-only is the default and mandatory for background jobs. The
     * foreground first-use workflow may deliberately request a new library. */
    fun acquire(context: Context, createIfMissing: Boolean = false): Pending {
        workerOnly()
        val root = context.noBackupFilesDir.canonicalFile
        val key = Root(root.path, context.applicationContext)
        return Pending(pool.acquire(key, createIfMissing))
    }
    private fun workerOnly() {
        check(Looper.myLooper() != Looper.getMainLooper()) { "Notebook runtime observation requires a worker" }
    }
}
