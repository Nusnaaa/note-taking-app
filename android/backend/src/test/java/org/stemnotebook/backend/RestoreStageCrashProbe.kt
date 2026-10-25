package org.stemnotebook.backend

import java.io.File
import java.io.FileInputStream

/** Developer-only child JVM. No Activity, owner data or device evidence. */
object RestoreStageCrashProbe {
    @JvmStatic fun main(args: Array<String>) {
        require(args.size == 3)
        val root = File(args[0])
        LibraryOwnership.acquire(root).use {
            val host = NativeBridge.openExisting(File(root, "notebook/library.db").absolutePath)
            val job = NativeBridge.beginRestore(host, 120_000)
            val input = FileInputStream(args[1])
            val summary = job.prepare("Deliberate restore cleanup test passphrase".toCharArray(), false,
                object : NativeBridge.RestoreInput {
                    override fun read(ciphertext: ByteArray): Int = input.read(ciphertext)
                    override fun retire() { input.close() }
                }, args[2], 16L * 1024 * 1024)
            check(summary.contains("\"entities\":1"))
            // Keep the validated private SQLite stage in the JNI registry; skip all destructors.
            Runtime.getRuntime().halt(73)
        }
        error("Crash boundary was not reached")
    }
}
