package org.stemnotebook.backend

import java.io.Closeable
import java.io.File
import java.io.RandomAccessFile
import java.nio.channels.FileLock
import java.util.concurrent.ExecutorService
import java.util.concurrent.TimeUnit

/** Cooperative app ownership precedes disposable-cache cleanup. Keep the lock file:
 * unlinking it would let another process lock a different inode at the same path. */
internal class LibraryOwnership private constructor(private val file: RandomAccessFile, private val lease: FileLock, private val key: String) : Closeable {
    private var closed = false
    @Synchronized override fun close() {
        if (closed) return
        closed = true
        try { lease.release() } finally { file.close() }
        synchronized(owners) { owners.remove(key) }
    }
    companion object {
        private val owners = mutableSetOf<String>()
        fun acquire(root: File): LibraryOwnership {
            var opened: RandomAccessFile? = null
            var reserved: String? = null
            try {
                check(root.isDirectory || root.mkdirs())
                val canonical = root.canonicalFile
                val key = canonical.path
                // Reject before opening any duplicate FD: closing a failed
                // duplicate channel can release this process's POSIX lease.
                synchronized(owners) { check(owners.add(key)); reserved = key }
                val lock = File(canonical, ".backend-owner")
                require(!java.nio.file.Files.isSymbolicLink(lock.toPath()))
                val file = RandomAccessFile(lock, "rw"); opened = file
                val lease = file.channel.tryLock() ?: throw IllegalStateException()
                return LibraryOwnership(file, lease, key)
            } catch (_: Exception) {
                try { opened?.close() } catch (_: Exception) { }
                reserved?.let { key -> synchronized(owners) { owners.remove(key) } }
                throw IllegalStateException("Notebook library already owned or unavailable")
            }
        }
    }
}

/** Called only by the dedicated closer, never by a writer/control/main thread. */
internal fun awaitStopped(vararg executors: ExecutorService) {
    for (executor in executors) while (!executor.awaitTermination(1, TimeUnit.SECONDS)) { /* retain ownership until actual retirement */ }
}
