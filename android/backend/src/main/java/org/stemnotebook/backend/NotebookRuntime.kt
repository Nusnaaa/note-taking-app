package org.stemnotebook.backend

import android.content.Context
import android.content.ContextWrapper
import android.os.Looper
import java.io.Closeable
import java.io.File
import java.util.concurrent.CompletableFuture
import java.util.concurrent.atomic.AtomicReference

/** Process-local shared backend entry point for foreground and scheduled work.
 * Acquire/await/canonical-path checks belong on a worker. No job is enabled here.
 * Legacy NotebookBackend constructors remain exclusive and cannot be adopted. */
object NotebookRuntime {
    internal class Root(val path: String, val application: Context, val guard: (() -> File)? = null) {
        override fun equals(other: Any?): Boolean = other is Root && path == other.path
        override fun hashCode(): Int = path.hashCode()
    }
    internal class Owner(val backend: NotebookBackend?, val ready: CompletableFuture<Void>, val closing: CompletableFuture<Void>,
        val released: CompletableFuture<Void>, val retire: () -> CompletableFuture<Void>)
    private val pool = SharedNotebookPool<Root, Owner>(
        { root, create ->
            val context = object : ContextWrapper(root.application) {
                override fun getNoBackupFilesDir(): File = root.guard?.invoke() ?: File(root.path)
            }
            val witness = AtomicReference<CompletableFuture<Void>?>()
            try {
                val backend = NotebookBackend(context, pdfFactory = { platform, native -> PdfWorkspace(platform, native) }, existingLibrary = !create,
                    runtimeOwnershipObserver = { witness.set(it) })
                Owner(backend, backend.runtimeReady(), backend.runtimeClosing(), backend.runtimeOwnershipReleased(), backend::closeFullyAsync)
            } catch (_: Exception) {
                val released: CompletableFuture<Void> = witness.get() ?: CompletableFuture.completedFuture<Void>(null)
                val failed = CompletableFuture<Void>().apply { completeExceptionally(SharedNotebookPool.Unavailable()) }
                Owner(null, failed, CompletableFuture.completedFuture<Void>(null), released) { released }
            }
        },
        { it.ready }, { it.closing }, { it.released }, { it.retire() },
    )
    class Pending internal constructor(private val request: SharedNotebookPool<Root, Owner>.Pending) : Closeable {
        fun await(): Lease { workerOnly(); return Lease(request.await()) }
        fun cancel(): Boolean = request.cancel()
        fun awaitRetired() { workerOnly(); request.awaitRetired() }
        override fun close() { request.close() }
    }
    class Lease internal constructor(private val lease: SharedNotebookPool<Root, Owner>.Lease) : Closeable {
        val backend: NotebookBackend get() = lease.value().backend ?: throw SharedNotebookPool.Unavailable()
        /** Release only after this consumer's operations have actually retired. */
        override fun close() { lease.close() }
        /** Worker-only observation. Other live leases keep the backend open. */
        fun awaitClosed() { workerOnly(); lease.awaitClosed() }
        fun awaitRetired() { workerOnly(); lease.awaitRetired() }
    }
    /** Existing-only is the default and mandatory for background jobs. The
     * foreground first-use workflow may deliberately request a new library. */
    fun acquire(context: Context, createIfMissing: Boolean = false): Pending {
        workerOnly()
        val root = context.noBackupFilesDir.canonicalFile
        val key = Root(root.path, context.applicationContext)
        return Pending(pool.acquire(key, createIfMissing))
    }
    /** Only the private scheduler map supplies this capability. Construction and
     * every backend root observation recheck its recorded filesystem identity. */
    internal fun acquireRouted(context: Context, route: BackupJobRoutes.Root): Pending {
        workerOnly(); route.file()
        return Pending(pool.acquire(Root(route.path, context.applicationContext, route::file), false))
    }
    private fun workerOnly() {
        check(Looper.myLooper() != Looper.getMainLooper()) { "Notebook runtime observation requires a worker" }
    }
}
