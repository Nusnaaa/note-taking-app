package org.stemnotebook.backend

import android.app.job.JobInfo
import android.app.job.JobScheduler
import android.content.ComponentName
import android.content.Context
import android.os.PersistableBundle
import java.io.IOException
import java.util.UUID

/** Only metadata goes into OS extras. Registration never grants notebook access. */
internal abstract class BackupJobScheduler {
    data class Token(val schedule: UUID, val revision: UUID)
    enum class Registration { Registered, Rejected, Uncertain }
    enum class Cancellation { Cancelled, Absent, Conflict, Uncertain }
    class Unavailable : IOException("Backup job scheduling unavailable")
    open fun supportsCurrentRoot(): Boolean = true
    abstract fun minimumPeriodMillis(): Long
    abstract fun register(configuration: BackupSchedulePolicy.Configuration): Registration
    abstract fun pending(): Token?
    abstract fun cancel(token: Token): Cancellation

    companion object {
        // Reserved by this app backend. A foreign occupant is never overwritten.
        const val JOB_ID = 0x53544201
        const val SERVICE = "org.stemnotebook.backend.NotebookBackupJobService"
        fun decode(jobId: Int, bundle: PersistableBundle): Token? = try {
            if (jobId != JOB_ID || bundle.keySet() != setOf("version", "schedule", "revision") || bundle.getInt("version", -1) != 1) null else {
                fun uuid(name: String): UUID {
                    val text = bundle.getString(name) ?: throw Unavailable()
                    require(text.length == 36)
                    return UUID.fromString(text).also { require(it.toString() == text) }
                }
                Token(uuid("schedule"), uuid("revision"))
            }
        } catch (_: Exception) { null }
        fun platform(context: Context): BackupJobScheduler = Platform(context)
    }
    private class Platform(context: Context) : BackupJobScheduler() {
        private val component = ComponentName(context.packageName, SERVICE)
        private val suppliedRoot = context.noBackupFilesDir
        private val primaryRoot = context.applicationContext.noBackupFilesDir
        override fun supportsCurrentRoot(): Boolean = suppliedRoot.canonicalFile == primaryRoot.canonicalFile
        private val scheduler = context.getSystemService(JobScheduler::class.java) ?: throw Unavailable()
        override fun minimumPeriodMillis(): Long = JobInfo.getMinPeriodMillis()
        private fun owned(job: JobInfo): Token? = if (job.service == component) decode(job.id, job.extras) else null
        override fun pending(): Token? = try {
            scheduler.getPendingJob(JOB_ID)?.let { owned(it) ?: throw Unavailable() }
        } catch (_: Exception) { throw Unavailable() }
        override fun register(configuration: BackupSchedulePolicy.Configuration): Registration {
            try {
                require(supportsCurrentRoot())
                require(configuration.state == BackupSchedulePolicy.State.Arming && configuration.maximumBytes?.let { it > 0 } == true)
                require(configuration.intervalMillis >= minimumPeriodMillis())
                val prior = scheduler.getPendingJob(JOB_ID)
                if (prior != null) require(owned(prior)?.schedule == configuration.id)
            } catch (_: Exception) { throw Unavailable() }
            val extras = PersistableBundle().apply {
                putInt("version", 1); putString("schedule", configuration.id.toString()); putString("revision", configuration.revision.toString())
            }
            val job = JobInfo.Builder(JOB_ID, component).setExtras(extras)
                .setPeriodic(configuration.intervalMillis).setPersisted(true)
                .setRequiredNetworkType(JobInfo.NETWORK_TYPE_NONE).build()
            return try {
                if (scheduler.schedule(job) == JobScheduler.RESULT_SUCCESS) Registration.Registered else Registration.Rejected
            } catch (error: Exception) {
                if (error is InterruptedException) Thread.currentThread().interrupt()
                Registration.Uncertain
            }
        }
        override fun cancel(token: Token): Cancellation {
            val current = try { scheduler.getPendingJob(JOB_ID) } catch (_: Exception) { throw Unavailable() }
                ?: return Cancellation.Absent
            if (owned(current) != token) return Cancellation.Conflict
            return try {
                scheduler.cancel(JOB_ID)
                if (scheduler.getPendingJob(JOB_ID) == null) Cancellation.Cancelled else Cancellation.Uncertain
            } catch (error: Exception) {
                if (error is InterruptedException) Thread.currentThread().interrupt()
                Cancellation.Uncertain
            }
        }
    }
}
