package org.stemnotebook.backend

import android.app.job.JobParameters
import android.content.Context
import android.content.ContextWrapper
import android.os.Looper
import android.os.PersistableBundle
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows
import org.robolectric.annotation.Config
import org.robolectric.shadow.api.Shadow
import org.robolectric.util.ReflectionHelpers
import java.io.File
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

@RunWith(JniBackendRunner::class)
@Config(sdk = [35])
class NotebookBackupJobServiceTest {
    @get:Rule val temporary = TemporaryFolder()
    class Service : NotebookBackupJobService() {
        var platform: Context? = null
        override fun getApplicationContext(): Context = platform ?: super.getApplicationContext()
    }
    private fun parameters(token: BackupJobScheduler.Token = BackupJobScheduler.Token(UUID.randomUUID(), UUID.randomUUID())): JobParameters {
        val result = Shadow.newInstanceOf(JobParameters::class.java)
        ReflectionHelpers.setField(result, "jobId", BackupJobScheduler.JOB_ID)
        ReflectionHelpers.setField(result, "extras", PersistableBundle().apply {
            putInt("version", 1); putString("schedule", token.schedule.toString()); putString("revision", token.revision.toString())
        })
        return result
    }
    private fun await(predicate: () -> Boolean) {
        val until = System.nanoTime() + TimeUnit.SECONDS.toNanos(15)
        while (!predicate()) {
            Shadows.shadowOf(Looper.getMainLooper()).idle()
            check(System.nanoTime() < until) { "Controlled service work did not retire" }
            Thread.sleep(1)
        }
    }
    @Test fun missingLibraryFinishesWithoutCreatingANotebookAndNeverOpensOnMain() {
        val root = temporary.newFolder(); val mainRead = AtomicBoolean(); val controller = Robolectric.buildService(Service::class.java).create(); val service = controller.get()
        service.platform = object : ContextWrapper(RuntimeEnvironment.getApplication()) {
            override fun getNoBackupFilesDir(): File { mainRead.set(Looper.myLooper() == Looper.getMainLooper()); return root }
        }
        try {
            assertTrue(service.onStartJob(parameters()))
            await { Shadows.shadowOf(service).isJobFinished }
            assertFalse(Shadows.shadowOf(service).isRescheduleNeeded); assertFalse(mainRead.get())
            assertFalse(File(root, "notebook/library.db").exists())
            LibraryOwnership.acquire(root).close()
        } finally { controller.destroy() }
    }
    @Test fun stopUsesTokenAcrossDistinctParameterObjectsAndRetainsBlockedWorkerUntilItRetires() {
        val root = temporary.newFolder(); val entered = CountDownLatch(1); val release = CountDownLatch(1)
        val controller = Robolectric.buildService(Service::class.java).create(); val service = controller.get()
        val interrupted = AtomicBoolean()
        service.platform = object : ContextWrapper(RuntimeEnvironment.getApplication()) {
            override fun getNoBackupFilesDir(): File {
                entered.countDown()
                try { check(release.await(15, TimeUnit.SECONDS)) } catch (error: InterruptedException) { interrupted.set(true); throw error }
                return root
            }
        }
        val token = BackupJobScheduler.Token(UUID.randomUUID(), UUID.randomUUID())
        try {
            assertTrue(service.onStartJob(parameters(token))); assertTrue(entered.await(10, TimeUnit.SECONDS))
            assertFalse(service.onStopJob(parameters(token))) // A newly parcelled object, same job token.
            assertFalse(Shadows.shadowOf(service).isJobFinished)
            assertFalse(service.onStartJob(parameters(token))) // Old worker still owns the service slot.
            release.countDown()
            await { ReflectionHelpers.getField<Any?>(service, "active") == null }
            assertFalse(Shadows.shadowOf(service).isJobFinished); assertFalse(interrupted.get())
            assertFalse(File(root, "notebook/library.db").exists())
            LibraryOwnership.acquire(root).close()
        } finally { release.countDown(); controller.destroy() }
    }
    @Test fun malformedExtrasNeverStartNotebookWork() {
        val controller = Robolectric.buildService(Service::class.java).create(); val service = controller.get(); val reads = AtomicBoolean()
        service.platform = object : ContextWrapper(RuntimeEnvironment.getApplication()) {
            override fun getNoBackupFilesDir(): File { reads.set(true); return temporary.root }
        }
        try {
            val params = parameters(); params.extras.putString("password", "forbidden fixture field")
            assertFalse(service.onStartJob(params)); assertFalse(reads.get()); assertFalse(Shadows.shadowOf(service).isJobFinished)
        } finally { controller.destroy() }
    }
}
