package org.stemnotebook.backend

import android.content.ContentValues
import android.database.Cursor
import android.database.sqlite.SQLiteDatabase
import java.io.File
import java.util.UUID

/** Durable state machine only. No OS job is registered by this repository. The
 * controller supplies reviewed authority and holds the shared library lease. */
internal class BackupSchedulePolicy(directory: File, private val fault: (String) -> Unit = {}) {
    enum class Destination { LocalDirectory, ProviderTree }
    enum class State { Arming, Enabled, Disabled }
    enum class Result { Published, ProviderVerified, Uncertain, NeedsAuthorization, CredentialUnavailable, SourceChanged, RecoveryRequired, DestinationUnavailable, SpaceUnavailable, Cancelled, Interrupted, Failed }
    data class Configuration(val id: UUID, val library: UUID, val revision: UUID, val state: State,
        val kind: Destination, val destination: String, val credential: UUID, val permission: UUID?,
        val intervalMillis: Long, val originalInk: Boolean, val sourceAssets: Boolean,
        val consentedAtMillis: Long, val activeRun: UUID?)
    data class Run(val id: UUID, val schedule: UUID, val revision: UUID, val startedAtMillis: Long,
        val finishedAtMillis: Long?, val result: Result?, val publicationSequence: Long?)
    private val database = BackupPolicyDatabase(directory)
    private val columns = "id,library,revision,state,kind,destination,credential,permission,interval_ms,original_ink,source_assets,consented_ms,active_run"
    private val runColumns = "id,schedule,revision,started_ms,finished_ms,result,publication_sequence"

    /** Controller must query the platform minimum interval, review the exact
     * target and perform initial native backup before calling. No private timing
     * SLO or target-availability promise is introduced by intervalMillis. */
    fun arm(library: UUID, expectedRevision: UUID?, kind: Destination, destination: String,
        credential: UUID, permission: UUID?, intervalMillis: Long, originalInk: Boolean, sourceAssets: Boolean,
        confirmedScheduling: Boolean): Configuration = guarded {
        require(confirmedScheduling && intervalMillis > 0)
        validateDestination(kind, destination, permission)
        transaction { db ->
            val prior = configuration(db, library)
            require(prior?.revision == expectedRevision && prior?.activeRun == null)
            // Reconfiguration is a fresh reviewed decision. No old job or secret
            // reference is silently re-enabled by a historical receipt.
            requireCredential(db, library, credential)
            if (permission != null) requirePermission(db, library, permission, destination)
            val next = Configuration(prior?.id ?: UUID.randomUUID(), library, UUID.randomUUID(), State.Arming,
                kind, destination, credential, permission, intervalMillis, originalInk, sourceAssets,
                System.currentTimeMillis().coerceAtLeast(0), null)
            val values = values(next)
            if (prior == null) db.insertOrThrow("configurations", null, values)
            else require(db.update("configurations", values, "id=? AND revision=?", arrayOf(prior.id.toString(), prior.revision.toString())) == 1)
            next
        }
    }
    /** Invoke only after JobScheduler acknowledged the exact id+revision. If the
     * acknowledgement is lost, leave Arming and reconcile, never infer Enabled. */
    fun registrationAcknowledged(library: UUID, revision: UUID): Configuration = guarded {
        transaction { db ->
            val prior = configuration(db, library) ?: throw BackupPolicyStore.Unavailable()
            require(prior.revision == revision && prior.state == State.Arming && prior.activeRun == null)
            requireCredential(db, library, prior.credential)
            if (prior.permission != null) requirePermission(db, library, prior.permission, prior.destination)
            db.execSQL("UPDATE configurations SET state=? WHERE id=? AND revision=?", arrayOf(State.Enabled.name, prior.id.toString(), revision.toString()))
            prior.copy(state = State.Enabled)
        }
    }
    /** Durable disablement precedes any OS cancellation. Running work still owns
     * its active-run fence until actual retirement records an outcome. */
    fun disable(library: UUID, revision: UUID): Configuration = guarded {
        transaction { db ->
            val prior = configuration(db, library) ?: throw BackupPolicyStore.Unavailable()
            require(prior.revision == revision)
            val next = prior.copy(state = State.Disabled, revision = UUID.randomUUID())
            require(db.update("configurations", values(next), "id=? AND revision=?", arrayOf(prior.id.toString(), revision.toString())) == 1)
            next
        }
    }
    fun configuration(library: UUID): Configuration? = guarded { database.access(false) { configuration(it, library) } }
    /** A stale, disabled or already-running job gets no run. Recheck credentials
     * and recorded provider consent in the same transaction as admission. */
    fun start(library: UUID, schedule: UUID, revision: UUID): Run? = guarded {
        transaction { db ->
            val config = configuration(db, library)
            if (config == null || config.id != schedule || config.revision != revision || config.state != State.Enabled || config.activeRun != null) null
            else {
                requireCredential(db, library, config.credential)
                if (config.permission != null) requirePermission(db, library, config.permission, config.destination)
                val run = Run(UUID.randomUUID(), config.id, revision, System.currentTimeMillis().coerceAtLeast(0), null, null, null)
                db.execSQL("INSERT INTO runs(id,schedule,revision,started_ms) VALUES(?,?,?,?)", arrayOf(run.id.toString(), run.schedule.toString(), run.revision.toString(), run.startedAtMillis))
                db.execSQL("UPDATE configurations SET active_run=? WHERE id=? AND active_run IS NULL", arrayOf(run.id.toString(), schedule.toString()))
                run
            }
        }
    }
    /** Caller retires native/provider/secret work first. Publication sequence
     * refers to an independently acknowledged catalogue record; no path/secret
     * is copied into a run outcome. Uncertain never becomes a known success. */
    fun finish(library: UUID, run: Run, result: Result, publicationSequence: Long? = null): Run = guarded {
        require(publicationSequence == null || publicationSequence > 0)
        require(publicationSequence == null || result == Result.Published || result == Result.ProviderVerified)
        transaction { db ->
            val config = configuration(db, library) ?: throw BackupPolicyStore.Unavailable()
            val prior = run(db, run.id) ?: throw BackupPolicyStore.Unavailable()
            require(prior == run && prior.result == null && config.id == run.schedule && config.activeRun == run.id)
            val done = prior.copy(finishedAtMillis = System.currentTimeMillis().coerceAtLeast(prior.startedAtMillis), result = result, publicationSequence = publicationSequence)
            db.execSQL("UPDATE runs SET finished_ms=?,result=?,publication_sequence=? WHERE id=? AND result IS NULL", arrayOf(done.finishedAtMillis, result.name, publicationSequence, run.id.toString()))
            db.execSQL("UPDATE configurations SET active_run=NULL WHERE id=? AND active_run=?", arrayOf(config.id.toString(), run.id.toString()))
            done
        }
    }
    /** Only after acquiring exclusive library ownership and proving no old
     * workers survive. Job stop/callback cancellation alone is insufficient. */
    fun reconcileInterrupted(library: UUID): Run? = guarded {
        transaction { db ->
            val config = configuration(db, library)
            val active = config?.activeRun
            if (active == null) null else {
                val prior = run(db, active) ?: throw BackupPolicyStore.Unavailable()
                require(prior.schedule == config.id && prior.result == null)
                val done = prior.copy(finishedAtMillis = System.currentTimeMillis().coerceAtLeast(prior.startedAtMillis), result = Result.Interrupted)
                db.execSQL("UPDATE runs SET finished_ms=?,result=? WHERE id=? AND result IS NULL", arrayOf(done.finishedAtMillis, Result.Interrupted.name, active.toString()))
                db.execSQL("UPDATE configurations SET active_run=NULL WHERE id=? AND active_run=?", arrayOf(config.id.toString(), active.toString()))
                done
            }
        }
    }
    fun runs(library: UUID, after: UUID? = null, limit: Int = 50): List<Run> = guarded {
        require(limit in 1..100)
        database.access(false) { db ->
            val config = configuration(db, library)
            if (config == null) emptyList() else db.rawQuery("SELECT $runColumns FROM runs WHERE schedule=? AND id>? ORDER BY id LIMIT ?",
                arrayOf(config.id.toString(), after?.toString() ?: "", limit.toString())).use { c -> buildList { while (c.moveToNext()) add(decodeRun(c)) } }
        }
    }
    private fun configuration(db: SQLiteDatabase, library: UUID): Configuration? = db.rawQuery("SELECT $columns FROM configurations WHERE library=?", arrayOf(library.toString())).use { c ->
        if (!c.moveToFirst()) null else decode(c).also { require(it.library == library && !c.moveToNext()) }
    }
    private fun values(c: Configuration) = ContentValues().apply {
        put("id", c.id.toString()); put("library", c.library.toString()); put("revision", c.revision.toString()); put("state", c.state.name)
        put("kind", c.kind.name); put("destination", c.destination); put("credential", c.credential.toString()); put("permission", c.permission?.toString())
        put("interval_ms", c.intervalMillis); put("original_ink", if (c.originalInk) 1 else 0); put("source_assets", if (c.sourceAssets) 1 else 0)
        put("consented_ms", c.consentedAtMillis); put("active_run", c.activeRun?.toString())
    }
    private fun decode(c: Cursor): Configuration {
        for (i in 0..6) require(c.getType(i) == Cursor.FIELD_TYPE_STRING)
        for (i in listOf(8, 9, 10, 11)) require(c.getType(i) == Cursor.FIELD_TYPE_INTEGER)
        val value = Configuration(uuid(c.getString(0)), uuid(c.getString(1)), uuid(c.getString(2)), State.valueOf(c.getString(3)),
            Destination.valueOf(c.getString(4)), c.getString(5), uuid(c.getString(6)), nullableUuid(c, 7), c.getLong(8), boolean(c, 9), boolean(c, 10), c.getLong(11), nullableUuid(c, 12))
        require(value.intervalMillis > 0 && value.consentedAtMillis >= 0)
        validateDestination(value.kind, value.destination, value.permission)
        return value
    }
    private fun run(db: SQLiteDatabase, id: UUID): Run? = db.rawQuery("SELECT $runColumns FROM runs WHERE id=?", arrayOf(id.toString())).use { c ->
        if (!c.moveToFirst()) null else decodeRun(c).also { require(!c.moveToNext()) }
    }
    private fun decodeRun(c: Cursor): Run {
        for (i in 0..2) require(c.getType(i) == Cursor.FIELD_TYPE_STRING)
        require(c.getType(3) == Cursor.FIELD_TYPE_INTEGER && c.getLong(3) >= 0)
        val finished = if (c.isNull(4)) null else { require(c.getType(4) == Cursor.FIELD_TYPE_INTEGER && c.getLong(4) >= c.getLong(3)); c.getLong(4) }
        val result = if (c.isNull(5)) null else { require(c.getType(5) == Cursor.FIELD_TYPE_STRING); Result.valueOf(c.getString(5)) }
        val sequence = if (c.isNull(6)) null else { require(c.getType(6) == Cursor.FIELD_TYPE_INTEGER && c.getLong(6) > 0); c.getLong(6) }
        require((finished == null) == (result == null))
        require(sequence == null || result == Result.Published || result == Result.ProviderVerified)
        return Run(uuid(c.getString(0)), uuid(c.getString(1)), uuid(c.getString(2)), c.getLong(3), finished, result, sequence)
    }
    private fun requireCredential(db: SQLiteDatabase, library: UUID, id: UUID) {
        db.rawQuery("SELECT library,purpose,state,length(envelope) FROM credentials WHERE id=?", arrayOf(id.toString())).use { c ->
            require(c.moveToFirst() && c.getString(0) == library.toString() && c.getString(1) == BackupSecretContext.Purpose.SchedulePassword.name && c.getString(2) == BackupPolicyStore.CredentialState.Ready.name && c.getLong(3) in 39..2085 && !c.moveToNext())
        }
    }
    private fun requirePermission(db: SQLiteDatabase, library: UUID, id: UUID, destination: String) {
        db.rawQuery("SELECT library,tree,outcome,observed_read,observed_write FROM permissions WHERE id=?", arrayOf(id.toString())).use { c ->
            require(c.moveToFirst() && c.getString(0) == library.toString() && c.getString(1) == destination)
            require(c.getString(2) in setOf(BackupPersistentAccess.Outcome.Persisted.name, BackupPersistentAccess.Outcome.AlreadyHeld.name))
            require(boolean(c, 3) && boolean(c, 4) && !c.moveToNext())
        }
    }
    private fun validateDestination(kind: Destination, destination: String, permission: UUID?) {
        require(destination.length in 1..4096 && !destination.contains('\u0000'))
        when (kind) {
            Destination.LocalDirectory -> require(File(destination).isAbsolute && permission == null)
            Destination.ProviderTree -> {
                val uri = android.net.Uri.parse(destination)
                require(permission != null && uri.scheme == "content" && !uri.authority.isNullOrEmpty() && android.provider.DocumentsContract.isTreeUri(uri))
            }
        }
    }
    private fun nullableUuid(c: Cursor, index: Int): UUID? = if (c.isNull(index)) null else { require(c.getType(index) == Cursor.FIELD_TYPE_STRING); uuid(c.getString(index)) }
    private fun uuid(value: String): UUID { require(value.length == 36); return UUID.fromString(value).also { require(it.toString() == value) } }
    private fun boolean(c: Cursor, index: Int): Boolean { require(c.getType(index) == Cursor.FIELD_TYPE_INTEGER && c.getLong(index) in 0..1); return c.getInt(index) == 1 }
    private fun <T> transaction(action: (SQLiteDatabase) -> T): T = database.access(true) { db ->
        db.beginTransaction()
        val result = try { val value = action(db); fault("schedule-before-commit"); db.setTransactionSuccessful(); value } finally { db.endTransaction() }
        fault("schedule-after-commit"); result
    }
    private fun <T> guarded(action: () -> T): T = try { action() } catch (error: Exception) {
        if (error is InterruptedException) Thread.currentThread().interrupt()
        if (error is InterruptedException || error is java.util.concurrent.CancellationException)
            throw java.util.concurrent.CancellationException("Backup schedule operation cancelled")
        throw BackupPolicyStore.Unavailable()
    }
}
