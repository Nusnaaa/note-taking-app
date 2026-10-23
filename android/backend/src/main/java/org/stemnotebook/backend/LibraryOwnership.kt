package org.stemnotebook.backend

import java.io.Closeable
import java.io.File
import java.io.RandomAccessFile
import java.nio.channels.FileLock
import java.util.concurrent.ExecutorService
import java.util.concurrent.TimeUnit

/** Cooperative app ownership precedes disposable-cache cleanup. Keep the lock file:
 * unlinking it would let another process lock a different inode at the same path. */
internal class LibraryOwnership private constructor(private val file: RandomAccessFile, private val lease: FileLock) : Closeable {
    private var closed = false
    @Synchronized override fun close() {
        if (closed) return
        closed = true
        try { lease.release() } finally { file.close() }
    }
    companion object {
        fun acquire(root: File): LibraryOwnership {
            var opened: RandomAccessFile? = null
            try {
                check(root.isDirectory || root.mkdirs())
                val lock = File(root.canonicalFile, ".backend-owner")
                require(!java.nio.file.Files.isSymbolicLink(lock.toPath()))
                val file = RandomAccessFile(lock, "rw"); opened = file
                val lease = file.channel.tryLock() ?: throw IllegalStateException()
                return LibraryOwnership(file, lease)
            } catch (_: Exception) {
                try { opened?.close() } catch (_: Exception) { }
                throw IllegalStateException("Notebook library already owned or unavailable")
            }
        }
    }
}

/** Called only by the dedicated closer, never by a writer/control/main thread. */
internal fun awaitStopped(vararg executors: ExecutorService) {
    for (executor in executors) while (!executor.awaitTermination(1, TimeUnit.SECONDS)) { /* retain ownership until actual retirement */ }
}
