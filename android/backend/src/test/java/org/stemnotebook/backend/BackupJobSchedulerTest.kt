package org.stemnotebook.backend

import android.app.job.JobInfo
import android.app.job.JobScheduler
import android.content.ComponentName
import android.os.PersistableBundle
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows
import org.robolectric.annotation.Config
import java.util.UUID

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class BackupJobSchedulerTest {
    private val context get() = RuntimeEnvironment.getApplication()
    private fun configuration() = BackupSchedulePolicy.Configuration(UUID.randomUUID(), UUID.randomUUID(), UUID.randomUUID(),
        BackupSchedulePolicy.State.Arming, BackupSchedulePolicy.Destination.LocalDirectory, "/explicit/backups", UUID.randomUUID(),
        null, JobInfo.getMinPeriodMillis(), true, false, 1L, null, 12345, "synthetic-directory-identity")
    @Test fun registrationContainsOnlyVersionAndOpaqueReferencesAndUsesPlatformPeriod() {
        val adapter = BackupJobScheduler.platform(context); val config = configuration()
        assertEquals(JobInfo.getMinPeriodMillis(), adapter.minimumPeriodMillis())
        assertEquals(BackupJobScheduler.Registration.Registered, adapter.register(config))
        val job = context.getSystemService(JobScheduler::class.java).getPendingJob(BackupJobScheduler.JOB_ID)!!
        assertEquals(setOf("version", "schedule", "revision"), job.extras.keySet())
        assertEquals(BackupJobScheduler.Token(config.id, config.revision), adapter.pending())
        assertEquals(JobInfo.NETWORK_TYPE_NONE, job.networkType); assertTrue(job.isPersisted)
        assertEquals(config.intervalMillis, job.intervalMillis)
        assertEquals(ComponentName(context.packageName, BackupJobScheduler.SERVICE), job.service)
    }
    @Test fun unownedOrMalformedJobIsNeverOverwrittenOrCancelled() {
        val scheduler = context.getSystemService(JobScheduler::class.java)
        val adapter = BackupJobScheduler.platform(context); val config = configuration()
        for (component in listOf(ComponentName(context.packageName, "other.Service"), ComponentName(context.packageName, BackupJobScheduler.SERVICE))) {
            val original = JobInfo.Builder(BackupJobScheduler.JOB_ID, component).setMinimumLatency(1).build()
            scheduler.schedule(original)
            assertThrows(BackupJobScheduler.Unavailable::class.java) { adapter.register(config) }
            assertEquals(BackupJobScheduler.Cancellation.Conflict, adapter.cancel(BackupJobScheduler.Token(config.id, config.revision)))
            assertSame(original, scheduler.getPendingJob(BackupJobScheduler.JOB_ID))
            scheduler.cancel(BackupJobScheduler.JOB_ID)
        }
    }
    @Test fun cancelledOnlyWhenObservedOwnedTokenStillMatches() {
        val adapter = BackupJobScheduler.platform(context); val config = configuration()
        adapter.register(config)
        assertEquals(BackupJobScheduler.Cancellation.Conflict, adapter.cancel(BackupJobScheduler.Token(config.id, UUID.randomUUID())))
        assertEquals(BackupJobScheduler.Token(config.id, config.revision), adapter.pending())
        assertEquals(BackupJobScheduler.Cancellation.Cancelled, adapter.cancel(BackupJobScheduler.Token(config.id, config.revision)))
        assertNull(adapter.pending())
        assertEquals(BackupJobScheduler.Cancellation.Absent, adapter.cancel(BackupJobScheduler.Token(config.id, config.revision)))
    }
    @Test fun invalidPolicyCannotEnterOsRegistration() {
        val adapter = BackupJobScheduler.platform(context); val config = configuration()
        for (invalid in listOf(config.copy(state = BackupSchedulePolicy.State.Disabled), config.copy(intervalMillis = JobInfo.getMinPeriodMillis() - 1), config.copy(maximumBytes = null))) {
            assertThrows(BackupJobScheduler.Unavailable::class.java) { adapter.register(invalid) }
            assertNull(adapter.pending())
        }
    }
    @Test fun knownOsRejectionDoesNotBecomeRegistered() {
        val scheduler = context.getSystemService(JobScheduler::class.java)
        Shadows.shadowOf(scheduler).failOnJob(BackupJobScheduler.JOB_ID)
        assertEquals(BackupJobScheduler.Registration.Rejected, BackupJobScheduler.platform(context).register(configuration()))
    }
    @Test fun productionAdapterDoesNotRegisterAnUnroutableIndependentLibraryRoot() {
        val alternate = java.nio.file.Files.createTempDirectory("unroutable-backup-root").toFile()
        try {
            val wrapper = object : android.content.ContextWrapper(context) { override fun getNoBackupFilesDir() = alternate }
            val adapter = BackupJobScheduler.platform(wrapper)
            assertFalse(adapter.supportsCurrentRoot())
            assertThrows(BackupJobScheduler.Unavailable::class.java) { adapter.register(configuration()) }
        } finally { alternate.delete() }
    }
    @Test fun tokenDecoderRejectsExtraSecretFieldsForeignIdsAndMalformedUuids() {
        val config = configuration(); val bundle = PersistableBundle().apply {
            putInt("version", 1); putString("schedule", config.id.toString()); putString("revision", config.revision.toString())
        }
        assertNotNull(BackupJobScheduler.decode(BackupJobScheduler.JOB_ID, bundle))
        assertNull(BackupJobScheduler.decode(123, bundle))
        bundle.putString("password", "fixture that must never enter a real job")
        assertNull(BackupJobScheduler.decode(BackupJobScheduler.JOB_ID, bundle)); bundle.remove("password")
        bundle.putString("revision", "malformed")
        assertNull(BackupJobScheduler.decode(BackupJobScheduler.JOB_ID, bundle))
    }
}
