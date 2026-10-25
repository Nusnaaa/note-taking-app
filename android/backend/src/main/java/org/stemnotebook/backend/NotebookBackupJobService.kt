package org.stemnotebook.backend

import android.app.job.JobParameters
import android.app.job.JobService
import android.os.Handler
import android.os.Looper
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit

/** OS callbacks never open SQLite or await native/provider/key work on main.
 * Stopping removes the OS observation, not the actual resource lifetime. */
open class NotebookBackupJobService : JobService() {
    private class Run(val parameters: JobParameters, val token: BackupJobScheduler.Token) {
        var stopped = false
        var opening: NotebookRuntime.Pending? = null
        var operation: NotebookSchedules.Pending<NotebookSchedules.RunReceipt>? = null
    }
    private val lock = Any()
    private val activeRuns = mutableMapOf<Int, Run>()
    private var destroyed = false
    private val main = Handler(Looper.getMainLooper())
    private val worker = ThreadPoolExecutor(1, 1, 0, TimeUnit.MILLISECONDS, ArrayBlockingQueue<Runnable>(BackupJobScheduler.JOB_SLOTS),
        { action -> Thread(action, "stem-backup-job").apply { isDaemon = true } }, ThreadPoolExecutor.AbortPolicy())
    override fun onStartJob(parameters: JobParameters): Boolean {
        val token = BackupJobScheduler.decode(parameters.jobId, parameters.extras) ?: return false
        val run = Run(parameters, token)
        synchronized(lock) {
            if (destroyed || activeRuns.containsKey(parameters.jobId) || activeRuns.size >= BackupJobScheduler.JOB_SLOTS) return false
            activeRuns[parameters.jobId] = run
            try { worker.execute { execute(run) } }
            catch (_: Exception) { activeRuns.remove(parameters.jobId); return false }
        }
        return true
    }
    private fun execute(run: Run) {
        var lease: NotebookRuntime.Lease? = null
        var opening: NotebookRuntime.Pending? = null
        try {
            synchronized(lock) { if (run.stopped) return }
            val platform = applicationContext
            val route = BackupJobRoutes.forApplication(platform).resolve(run.parameters.jobId, run.token)
            synchronized(lock) { if (run.stopped) return }
            opening = NotebookRuntime.acquireRouted(platform, route.root)
            synchronized(lock) { run.opening = opening; if (run.stopped) opening.cancel() }
            lease = opening.use { it.await() }
            synchronized(lock) { run.opening = null; if (run.stopped) return }
            val operation = lease.backend.schedules().runDue(run.token, route.library)
            synchronized(lock) { run.operation = operation; if (run.stopped) operation.cancel() }
            try { operation.await() } finally { operation.close(); operation.awaitRetired() }
        } catch (_: Exception) {
            // No notebook payload/credentials or unverified cause enters logs.
            // Persisted controller outcomes describe any admitted run.
        } finally {
            try { lease?.close(); lease?.awaitClosed() } catch (_: Exception) { }
            lease?.awaitRetired()
            opening?.let { it.close(); it.awaitRetired() }
            main.post {
                val finish = synchronized(lock) {
                    if (activeRuns[run.parameters.jobId] === run) activeRuns.remove(run.parameters.jobId)
                    !destroyed && !run.stopped
                }
                if (finish) jobFinished(run.parameters, false)
            }
        }
    }
    override fun onStopJob(parameters: JobParameters): Boolean {
        val token = BackupJobScheduler.decode(parameters.jobId, parameters.extras) ?: return false
        val run = synchronized(lock) {
            activeRuns[parameters.jobId]?.takeIf { it.token == token }?.also { it.stopped = true }
        }
        run?.opening?.cancel(); run?.operation?.cancel()
        // The periodic registration remains. No immediate retry storm, and the
        // worker retains its lease until actual native/provider/key retirement.
        return false
    }
    override fun onDestroy() {
        val runs = synchronized(lock) { destroyed = true; activeRuns.values.toList().onEach { it.stopped = true } }
        runs.forEach { it.opening?.cancel(); it.operation?.cancel() }
        worker.shutdown() // Never interrupt/abandon an admitted native callback.
        super.onDestroy()
    }
}
