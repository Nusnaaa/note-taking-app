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
    private fun parameters(token: BackupJobScheduler.Token = BackupJobScheduler.Token(UUID.randomUUID(), UUID.randomUUID()), jobId: Int = BackupJobScheduler.JOB_ID): JobParameters {
        val result = Shadow.newInstanceOf(JobParameters::class.java)
        ReflectionHelpers.setField(result, "jobId", jobId)
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
            await { ReflectionHelpers.getField<Map<Int, *>>(service, "activeRuns").isEmpty() }
            assertFalse(Shadows.shadowOf(service).isJobFinished); assertFalse(interrupted.get())
            assertFalse(File(root, "notebook/library.db").exists())
            LibraryOwnership.acquire(root).close()
        } finally { release.countDown(); controller.destroy() }
    }
    @Test fun distinctJobsQueueAndStoppingOneTokenCannotStopAnotherJobWithTheSameReferences() {
        val root = temporary.newFolder(); val entered = CountDownLatch(1); val release = CountDownLatch(1)
        val reads = java.util.concurrent.atomic.AtomicInteger()
        val controller = Robolectric.buildService(Service::class.java).create(); val service = controller.get()
        service.platform = object : ContextWrapper(RuntimeEnvironment.getApplication()) {
            override fun getNoBackupFilesDir(): File {
                reads.incrementAndGet(); entered.countDown(); check(release.await(15, TimeUnit.SECONDS)); return root
            }
        }
        val token = BackupJobScheduler.Token(UUID.randomUUID(), UUID.randomUUID())
        try {
            assertTrue(service.onStartJob(parameters(token))); assertTrue(entered.await(10, TimeUnit.SECONDS))
            assertTrue(service.onStartJob(parameters(token, BackupJobScheduler.JOB_ID + 1)))
            assertFalse(service.onStopJob(parameters(token, BackupJobScheduler.JOB_ID + 1)))
            val runs = ReflectionHelpers.getField<Map<Int, Any>>(service, "activeRuns")
            assertEquals(2, runs.size)
            assertFalse(ReflectionHelpers.getField<Boolean>(runs.getValue(BackupJobScheduler.JOB_ID), "stopped"))
            release.countDown()
            await { ReflectionHelpers.getField<Map<Int, *>>(service, "activeRuns").isEmpty() }
            assertEquals(1, reads.get()) // Cancelled queued work never resolves or opens a root.
            assertFalse(File(root, "notebook/library.db").exists())
        } finally { release.countDown(); controller.destroy() }
    }
    @Test fun unmappedIndependentJobNeverFallsBackToPrimaryLibrary() {
        val root = temporary.newFolder(); val controller = Robolectric.buildService(Service::class.java).create(); val service = controller.get()
        service.platform = object : ContextWrapper(RuntimeEnvironment.getApplication()) { override fun getNoBackupFilesDir() = root }
        try {
            assertTrue(service.onStartJob(parameters(jobId = BackupJobScheduler.JOB_ID + 1)))
            await { Shadows.shadowOf(service).isJobFinished }
            assertFalse(File(root, "notebook").exists()); assertFalse(File(root, ".backend-owner").exists())
            assertFalse(File(root, "backup-job-routes").exists())
        } finally { controller.destroy() }
    }
    @Test fun mappedIndependentJobSharesForegroundRuntimeAndRecordsCredentialFailureInItsLibrary() {
        val primary = temporary.newFolder(); val alternate = temporary.newFolder(); val destination = temporary.newFolder()
        val notebook = File(alternate, "notebook").apply { check(mkdir()) }
        val native = NativeBridge.open(File(notebook, "library.db").path)
        val library = try { UUID.fromString(NativeBridge.captureLibraryIdentity(native)) } finally { NativeBridge.close(native) }
        val policyDirectory = File(notebook, "backup-policy"); val store = BackupPolicyStore(policyDirectory)
        val password = BackupSecretContext(library, UUID.randomUUID(), BackupSecretContext.Purpose.SchedulePassword)
        store.beginCredential(password, null, true, true)
        // Structural envelope only. This routing test intentionally exercises
        // unavailable device credentials, not a real Keystore/publication claim.
        store.commitCredential(password, "STEMKEY1".toByteArray() + byteArrayOf(1) + ByteArray(30))
        val attributes = java.nio.file.Files.readAttributes(destination.toPath(), "unix:dev,ino")
        val identity = "${attributes.getValue("dev")}:${attributes.getValue("ino")}"
        val policy = BackupSchedulePolicy(policyDirectory)
        val arming = policy.arm(library, null, BackupSchedulePolicy.Destination.LocalDirectory, destination.path, password.id,
            null, android.app.job.JobInfo.getMinPeriodMillis(), true, false, true, 1000000, identity)
        val application = object : ContextWrapper(RuntimeEnvironment.getApplication()) {
            override fun getNoBackupFilesDir() = primary
            override fun getApplicationContext(): Context = this
        }
        val source = object : ContextWrapper(application) { override fun getNoBackupFilesDir() = alternate }
        // The production adapter calls the synthetic framework JobScheduler.
        assertEquals(BackupJobScheduler.Registration.Registered, BackupJobScheduler.platform(source).register(arming))
        val enabled = policy.registrationAcknowledged(library, arming.revision)
        val token = BackupJobScheduler.Token(enabled.id, enabled.revision)
        val route = BackupJobRoutes(primary).root(alternate)!!
        val controller = Robolectric.buildService(Service::class.java).create(); val service = controller.get()
        service.platform = application
        val observations = java.util.concurrent.Executors.newSingleThreadExecutor()
        val foreground = observations.submit<NotebookRuntime.Lease> { NotebookRuntime.acquire(source).use { it.await() } }.get(15, TimeUnit.SECONDS)
        try {
            assertTrue(service.onStartJob(parameters(token, route.job)))
            await { Shadows.shadowOf(service).isJobFinished }
            val run = policy.runs(library).single()
            assertEquals(BackupSchedulePolicy.Result.CredentialUnavailable, run.result)
            assertNull(policy.configuration(library)!!.activeRun)
            assertTrue(destination.listFiles()!!.isEmpty()); assertFalse(File(primary, "notebook").exists())
            assertTrue(org.json.JSONObject(foreground.backend.request("""{"version":1,"command":{"op":"Status"}}""").get(10, TimeUnit.SECONDS)).getBoolean("ok"))
            assertThrows(IllegalStateException::class.java) { LibraryOwnership.acquire(alternate) }
        } finally {
            controller.destroy()
            observations.submit { foreground.close(); foreground.awaitRetired() }.get(15, TimeUnit.SECONDS)
            observations.shutdownNow()
        }
        LibraryOwnership.acquire(alternate).close()
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
