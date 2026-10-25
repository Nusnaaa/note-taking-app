package org.stemnotebook.backend

import android.content.ContextWrapper
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import java.io.File
import java.nio.file.Files
import java.util.concurrent.Callable
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

@RunWith(JniBackendRunner::class)
@Config(sdk = [35])
class NotebookRuntimeTest {
    @get:Rule val temporary = TemporaryFolder()
    private fun context(root: File) = object : ContextWrapper(RuntimeEnvironment.getApplication()) { override fun getNoBackupFilesDir() = root }
    private fun <T> worker(action: () -> T): T {
        val executor = Executors.newSingleThreadExecutor()
        return try { executor.submit(Callable { action() }).get(20, TimeUnit.SECONDS) } finally { executor.shutdownNow() }
    }
    private fun status(backend: NotebookBackend): JSONObject = JSONObject(backend.request("""{"version":1,"command":{"op":"Status"}}""").get(10, TimeUnit.SECONDS))
    @Test fun foregroundAndBackgroundLeasesShareActualNativeBackendUntilLastRelease(): Unit = worker {
        val root = temporary.newFolder(); val platform = context(root)
        val foreground = NotebookRuntime.acquire(platform, true).await()
        val background = NotebookRuntime.acquire(platform).await()
        try {
            assertSame(foreground.backend, background.backend)
            assertTrue(status(background.backend).getBoolean("ok"))
            foreground.close(); foreground.awaitClosed()
            assertThrows(SharedNotebookPool.Unavailable::class.java) { foreground.backend }
            assertThrows(IllegalStateException::class.java) { LibraryOwnership.acquire(root) }
            assertTrue(status(background.backend).getBoolean("ok"))
        } finally { foreground.close(); background.close(); background.awaitClosed() }
        val reopened = NotebookRuntime.acquire(platform).await()
        try { assertTrue(status(reopened.backend).getBoolean("ok")) } finally { reopened.close(); reopened.awaitClosed() }
    }
    @Test fun canonicalAliasesReuseSameBackendWithoutOpeningDuplicateDescriptors(): Unit = worker {
        val root = temporary.newFolder(); val alias = File(temporary.root, "alias")
        Files.createSymbolicLink(alias.toPath(), root.toPath())
        val first = NotebookRuntime.acquire(context(root), true).await()
        val aliased = NotebookRuntime.acquire(context(alias)).await()
        try { assertSame(first.backend, aliased.backend); assertTrue(status(aliased.backend).getBoolean("ok")) }
        finally { first.close(); aliased.close(); aliased.awaitClosed() }
    }
    @Test fun existingOnlyOpeningDoesNotCreateAnEmptyNotebook(): Unit = worker {
        val root = temporary.newFolder()
        val pending = NotebookRuntime.acquire(context(root))
        try { assertThrows(SharedNotebookPool.Unavailable::class.java) { pending.await() } }
        finally { pending.close() }
        assertFalse(File(root, "notebook/library.db").exists())
        val until = System.nanoTime() + TimeUnit.SECONDS.toNanos(10)
        while (true) {
            try { LibraryOwnership.acquire(root).close(); break } catch (_: IllegalStateException) { }
            check(System.nanoTime() < until); Thread.sleep(1)
        }
    }
    @Test fun directExclusiveOwnerIsNeverAdoptedOrDisplaced(): Unit = worker {
        val root = temporary.newFolder(); val platform = context(root); val direct = NotebookBackend(platform)
        try {
            assertTrue(status(direct).getBoolean("ok"))
            val request = NotebookRuntime.acquire(platform)
            try { assertThrows(SharedNotebookPool.Unavailable::class.java) { request.await() } } finally { request.close() }
            assertTrue(status(direct).getBoolean("ok"))
            assertThrows(IllegalStateException::class.java) { LibraryOwnership.acquire(root) }
        } finally { direct.closeFullyAsync().get(10, TimeUnit.SECONDS) }
    }
    @Test fun mainThreadCannotStartBlockingRuntimeObservation() {
        assertThrows(IllegalStateException::class.java) { NotebookRuntime.acquire(context(temporary.newFolder()), true) }
    }
}
