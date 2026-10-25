package org.stemnotebook.backend

import android.content.ContentValues
import android.database.Cursor
import android.database.sqlite.SQLiteDatabase
import android.net.Uri
import java.io.File
import java.io.IOException
import java.util.UUID

/** Durable authority records only: writing an intent never enables a job. Caller
 * must hold library ownership, fresh reviewed consent and the optional worker. */
internal class BackupPolicyStore(directory: File, private val fault: (String) -> Unit = {}) {
    class Unavailable : IOException("Backup policy unavailable")
    enum class CredentialState { Intent, Ready, Retired, Deleted }
    data class Credential(val context: BackupSecretContext, val backup: UUID?, val state: CredentialState,
        val consentedAtMillis: Long, val envelope: ByteArray?)
    private val database = BackupPolicyDatabase(directory)

    /** Persist before creating a Keystore alias. A lost acknowledgement leaves a
     * discoverable intent; fresh UUIDs and tombstones prevent reference reuse. */
    fun beginCredential(context: BackupSecretContext, backup: UUID?, confirmedDeviceStorage: Boolean,
        independentRecoveryAcknowledged: Boolean): Credential = guarded {
        require(confirmedDeviceStorage && independentRecoveryAcknowledged)
        require((context.purpose == BackupSecretContext.Purpose.RecoveryKey) == (backup != null))
        val now = System.currentTimeMillis().coerceAtLeast(0)
        transaction { db ->
            val values = ContentValues().apply {
                put("id", context.id.toString()); put("library", context.library.toString()); put("purpose", context.purpose.name)
                put("backup", backup?.toString()); put("state", CredentialState.Intent.name); put("consented_ms", now); putNull("envelope")
            }
            db.insertOrThrow("credentials", null, values)
        }
        Credential(context, backup, CredentialState.Intent, now, null)
    }
    /** Only a complete purpose-bound encrypted envelope may replace an intent.
     * AEAD authenticity is checked by BackupSecretProtector before consumption. */
    fun commitCredential(context: BackupSecretContext, envelope: ByteArray) = guarded {
        validateEnvelope(envelope)
        transaction { db ->
            val old = find(db, context) ?: throw Unavailable()
            require(old.state == CredentialState.Intent)
            val values = ContentValues().apply { put("state", CredentialState.Ready.name); put("envelope", envelope) }
            require(db.update("credentials", values, "id=? AND state=?", arrayOf(context.id.toString(), CredentialState.Intent.name)) == 1)
        }
    }
    fun credential(context: BackupSecretContext): Credential? = guarded { database.access(false) { find(it, context) } }
    fun credentials(library: UUID, after: UUID? = null, limit: Int = 50): List<Credential> = guarded {
        require(limit in 1..100)
        database.access(false) { db ->
            db.rawQuery("SELECT id,library,purpose,backup,state,consented_ms,CASE WHEN envelope IS NULL THEN NULL WHEN length(envelope)<=2085 THEN envelope ELSE zeroblob(0) END FROM credentials WHERE library=? AND id>? ORDER BY id LIMIT ?",
                arrayOf(library.toString(), after?.toString() ?: "", limit.toString())).use { c -> buildList { while (c.moveToNext()) add(decode(c)) } }
        }
    }
    /** Durable denial precedes key retirement. Ciphertext is kept until actual
     * deletion acknowledges, including on restart or a lost delete response. */
    fun markRetired(context: BackupSecretContext) = guarded {
        transaction { db ->
            val old = find(db, context) ?: throw Unavailable()
            if (old.state != CredentialState.Deleted && old.state != CredentialState.Retired) {
                db.execSQL("UPDATE configurations SET state=?,revision=? WHERE library=? AND credential=? AND state<>?",
                    arrayOf(BackupSchedulePolicy.State.Disabled.name, UUID.randomUUID().toString(), context.library.toString(), context.id.toString(), BackupSchedulePolicy.State.Disabled.name))
                db.execSQL("UPDATE credentials SET state=? WHERE id=?", arrayOf(CredentialState.Retired.name, context.id.toString()))
            }
        }
    }
    fun acknowledgeDeleted(context: BackupSecretContext) = guarded {
        transaction { db ->
            val old = find(db, context) ?: throw Unavailable()
            require(old.state == CredentialState.Retired || old.state == CredentialState.Deleted)
            db.execSQL("UPDATE credentials SET state=?,envelope=NULL WHERE id=?", arrayOf(CredentialState.Deleted.name, context.id.toString()))
        }
    }
    /** Call inside the protector lease immediately before exposing plaintext. */
    fun requireReady(context: BackupSecretContext) {
        if (credential(context)?.state != CredentialState.Ready) throw Unavailable()
    }
    fun recordPermissionDecision(decision: BackupPersistentAccess.Decision) = guarded {
        validateDecision(decision)
        transaction { db ->
            val values = ContentValues().apply {
                put("id", decision.id.toString()); put("library", decision.library.toString()); put("tree", decision.tree)
                put("identity", decision.identity); put("previous_read", if (decision.previousRead) 1 else 0)
                put("previous_write", if (decision.previousWrite) 1 else 0); put("consented_ms", decision.consentedAtMillis)
            }
            db.insertOrThrow("permissions", null, values)
        }
    }
    fun recordPermissionOutcome(receipt: BackupPersistentAccess.Receipt) = guarded {
        validateDecision(receipt.decision)
        transaction { db ->
            val prior = permission(db, receipt.decision.id) ?: throw Unavailable()
            require(prior.first == receipt.decision && prior.second == null)
            require((receipt.observedRead == null) == (receipt.observedWrite == null))
            if (receipt.outcome != BackupPersistentAccess.Outcome.Uncertain) require(receipt.observedRead == true && receipt.observedWrite == true)
            val values = ContentValues().apply {
                put("outcome", receipt.outcome.name)
                if (receipt.observedRead == null) putNull("observed_read") else put("observed_read", if (receipt.observedRead) 1 else 0)
                if (receipt.observedWrite == null) putNull("observed_write") else put("observed_write", if (receipt.observedWrite) 1 else 0)
            }
            require(db.update("permissions", values, "id=? AND outcome IS NULL", arrayOf(receipt.decision.id.toString())) == 1)
        }
    }
    fun permission(id: UUID): Pair<BackupPersistentAccess.Decision, BackupPersistentAccess.Receipt?>? = guarded {
        database.access(false) { permission(it, id) }
    }
    fun permissions(library: UUID, after: UUID? = null, limit: Int = 50): List<Pair<BackupPersistentAccess.Decision, BackupPersistentAccess.Receipt?>> = guarded {
        require(limit in 1..100)
        database.access(false) { db ->
            val ids = db.rawQuery("SELECT id FROM permissions WHERE library=? AND id>? ORDER BY id LIMIT ?",
                arrayOf(library.toString(), after?.toString() ?: "", limit.toString())).use { c ->
                buildList { while (c.moveToNext()) { require(c.getType(0) == Cursor.FIELD_TYPE_STRING); add(uuid(c.getString(0))) } }
            }
            ids.map { permission(db, it) ?: throw Unavailable() }
        }
    }
    private fun permission(db: SQLiteDatabase, id: UUID): Pair<BackupPersistentAccess.Decision, BackupPersistentAccess.Receipt?>? {
        db.rawQuery("SELECT library,tree,identity,previous_read,previous_write,consented_ms,outcome,observed_read,observed_write FROM permissions WHERE id=?", arrayOf(id.toString())).use { c ->
            if (!c.moveToFirst()) return null
            for (i in 0..2) require(c.getType(i) == Cursor.FIELD_TYPE_STRING)
            require(c.getType(5) == Cursor.FIELD_TYPE_INTEGER)
            val decision = BackupPersistentAccess.Decision(id, uuid(c.getString(0)), c.getString(1), c.getString(2), bool(c, 3)!!, bool(c, 4)!!, c.getLong(5))
            validateDecision(decision)
            val read = bool(c, 7); val write = bool(c, 8)
            val receipt = if (c.isNull(6)) { require(read == null && write == null); null } else {
                require(c.getType(6) == Cursor.FIELD_TYPE_STRING && (read == null) == (write == null))
                val outcome = BackupPersistentAccess.Outcome.valueOf(c.getString(6))
                if (outcome != BackupPersistentAccess.Outcome.Uncertain) require(read == true && write == true)
                BackupPersistentAccess.Receipt(decision, outcome, read, write)
            }
            require(!c.moveToNext()); return decision to receipt
        }
    }
    private fun bool(c: Cursor, index: Int): Boolean? {
        if (c.isNull(index)) { return null }
        require(c.getType(index) == Cursor.FIELD_TYPE_INTEGER && c.getLong(index) in 0..1)
        return c.getInt(index) == 1
    }
    private fun validateDecision(d: BackupPersistentAccess.Decision) {
        require(d.consentedAtMillis >= 0 && d.tree.length in 1..4096 && d.identity.length in 1..4096)
        require(!d.tree.contains('\u0000') && !d.identity.contains('\u0000'))
        val uri = Uri.parse(d.tree)
        require(uri.scheme == "content" && !uri.authority.isNullOrEmpty() && android.provider.DocumentsContract.isTreeUri(uri))
        require(android.provider.DocumentsContract.getTreeDocumentId(uri) == d.identity)
    }
    private fun find(db: SQLiteDatabase, context: BackupSecretContext): Credential? =
        db.rawQuery("SELECT id,library,purpose,backup,state,consented_ms,CASE WHEN envelope IS NULL THEN NULL WHEN length(envelope)<=2085 THEN envelope ELSE zeroblob(0) END FROM credentials WHERE id=?", arrayOf(context.id.toString())).use { c ->
            if (!c.moveToFirst()) null else decode(c).also { require(it.context == context && !c.moveToNext()) }
        }
    private fun decode(c: Cursor): Credential {
        for (i in listOf(0, 1, 2, 4)) require(c.getType(i) == Cursor.FIELD_TYPE_STRING)
        require(c.getType(5) == Cursor.FIELD_TYPE_INTEGER && c.getLong(5) >= 0)
        val context = BackupSecretContext(uuid(c.getString(1)), uuid(c.getString(0)), BackupSecretContext.Purpose.valueOf(c.getString(2)))
        val backup = if (c.isNull(3)) null else { require(c.getType(3) == Cursor.FIELD_TYPE_STRING); uuid(c.getString(3)) }
        require((context.purpose == BackupSecretContext.Purpose.RecoveryKey) == (backup != null))
        val state = CredentialState.valueOf(c.getString(4))
        val envelope = if (c.isNull(6)) null else { require(c.getType(6) == Cursor.FIELD_TYPE_BLOB); c.getBlob(6).also { validateEnvelope(it) } }
        require(state != CredentialState.Ready || envelope != null)
        require(state != CredentialState.Intent && state != CredentialState.Deleted || envelope == null)
        return Credential(context, backup, state, c.getLong(5), envelope)
    }
    private fun validateEnvelope(value: ByteArray) {
        require(value.size in 39..2085 && (value.size - 37) % 2 == 0)
        require(value.copyOfRange(0, 8).contentEquals("STEMKEY1".toByteArray(Charsets.US_ASCII)) && value[8] == 1.toByte())
    }
    private fun uuid(value: String): UUID { require(value.length == 36); return UUID.fromString(value).also { require(it.toString() == value) } }
    private fun <T> transaction(action: (SQLiteDatabase) -> T): T = database.access(true) { db ->
        db.beginTransaction()
        val result = try { val value = action(db); fault("policy-before-commit"); db.setTransactionSuccessful(); value } finally { db.endTransaction() }
        fault("policy-after-commit"); result
    }
    private fun <T> guarded(action: () -> T): T = try { action() } catch (error: Exception) {
        if (error is InterruptedException) Thread.currentThread().interrupt()
        if (error is InterruptedException || error is java.util.concurrent.CancellationException)
            throw java.util.concurrent.CancellationException("Backup policy operation cancelled")
        throw Unavailable()
    }
}
