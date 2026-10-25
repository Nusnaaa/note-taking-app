package org.stemnotebook.backend

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.CancellationSignal
import android.provider.DocumentsContract
import java.io.Closeable
import java.io.IOException
import java.util.UUID

internal abstract class BackupPersistentPermissions {
    data class Held(val read: Boolean, val write: Boolean) { val complete get() = read && write }
    abstract fun current(tree: Uri): Held
    abstract fun take(tree: Uri)
    companion object {
        fun platform(context: Context): BackupPersistentPermissions = object : BackupPersistentPermissions() {
            private val resolver = context.applicationContext.contentResolver
            override fun current(tree: Uri): Held {
                var read = false; var write = false
                for (grant in resolver.persistedUriPermissions) if (grant.uri == tree) {
                    read = read || grant.isReadPermission; write = write || grant.isWritePermission
                }
                return Held(read, write)
            }
            override fun take(tree: Uri) = resolver.takePersistableUriPermission(tree,
                Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
        }
    }
}

/** Persistent destination access is distinct from enabling a job or storing a
 * credential. Only trusted picker/config capabilities enter this worker API.
 * Never release grants in rollback: another app feature may already use them. */
internal class BackupPersistentAccess(
    private val library: UUID,
    private val documents: BackupDocuments,
    private val permissions: BackupPersistentPermissions,
    private val authority: () -> Unit,
) {
    private val lock = Any()
    private var epoch = Any()
    private val owner = Any()
    class Unavailable : IOException("Backup destination consent unavailable")
    enum class Outcome { Persisted, AlreadyHeld, Uncertain }
    data class Decision(val id: UUID, val library: UUID, val tree: String, val identity: String,
        val previousRead: Boolean, val previousWrite: Boolean, val consentedAtMillis: Long)
    data class Receipt(val decision: Decision, val outcome: Outcome, val observedRead: Boolean?, val observedWrite: Boolean?)
    inner class Review internal constructor(internal val ownerToken: Any, internal val epochToken: Any,
        internal val tree: Uri, internal val selection: BackupDocuments.Selection,
        internal val previous: BackupPersistentPermissions.Held) : Closeable {
        internal var live = true
        val label: String get() = synchronized(lock) { check(live && epochToken === epoch); selection.label }
        override fun close() = synchronized(lock) { live = false }
    }
    fun invalidate() = synchronized(lock) { epoch = Any() }
    fun review(tree: Uri, pickerFlags: Int, signal: CancellationSignal, control: () -> Unit): Review = guarded {
        val observedEpoch = synchronized(lock) { epoch }
        control(); authority()
        require(tree.scheme == "content" && !tree.authority.isNullOrEmpty() && tree.toString().length <= 4096)
        val held = permissions.current(tree)
        if (!held.complete) {
            val required = Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
            require(pickerFlags and required == required)
        }
        val selection = documents.inspect(tree, signal)
        require(selection.identity == DocumentsContract.getTreeDocumentId(tree))
        require(selection.identity.length <= 4096 && selection.label.length <= 4096)
        control(); authority()
        synchronized(lock) {
            require(epoch === observedEpoch)
            Review(owner, observedEpoch, Uri.parse(tree.toString()), selection, held)
        }
    }
    /** Record the explicit consent decision BEFORE requesting OS persistence.
     * After entering take(), errors/cancellation must retain an effect receipt.
     * A store acknowledgement failure is conservative Uncertain, never hidden. */
    fun accept(review: Review, confirmed: Boolean, allowPersistentAccess: Boolean,
        proof: (String) -> Boolean, signal: CancellationSignal, control: () -> Unit,
        recordDecision: (Decision) -> Unit, recordOutcome: (Receipt) -> Unit): Receipt {
        guarded {
            require(confirmed && allowPersistentAccess)
            synchronized(lock) {
                require(review.ownerToken === owner && review.epochToken === epoch && review.live)
                review.live = false
            }
            control(); authority()
            require(documents.inspect(review.tree, signal) == review.selection && permissions.current(review.tree) == review.previous)
            require(proof(UUID.randomUUID().toString()))
            control(); authority()
            synchronized(lock) { require(review.epochToken === epoch) }
            require(documents.inspect(review.tree, signal) == review.selection && permissions.current(review.tree) == review.previous)
        }
        val decision = Decision(UUID.randomUUID(), library, review.tree.toString(), review.selection.identity,
            review.previous.read, review.previous.write, System.currentTimeMillis().coerceAtLeast(0))
        guarded {
            recordDecision(decision); control(); authority()
            // The irreversible permission phase begins only while this reviewed
            // policy epoch is current. Later invalidation retains a receipt.
            synchronized(lock) { require(review.epochToken === epoch) }
        }
        var observed: BackupPersistentPermissions.Held? = null
        var outcome = if (review.previous.complete) Outcome.AlreadyHeld else Outcome.Persisted
        try {
            if (!review.previous.complete) permissions.take(review.tree)
            observed = permissions.current(review.tree)
            require(observed.complete)
            control(); authority()
            synchronized(lock) { require(review.epochToken === epoch) }
            require(documents.inspect(review.tree, signal) == review.selection)
        } catch (error: Exception) {
            if (error is InterruptedException) Thread.currentThread().interrupt()
            outcome = Outcome.Uncertain
            observed = try { permissions.current(review.tree) } catch (_: Exception) { null }
        }
        val result = Receipt(decision, outcome, observed?.read, observed?.write)
        return try { recordOutcome(result); result }
        catch (error: Exception) {
            if (error is InterruptedException) Thread.currentThread().interrupt()
            result.copy(outcome = Outcome.Uncertain)
        }
    }
    private fun <T> guarded(action: () -> T): T = try { action() } catch (error: Exception) {
        if (error is InterruptedException) Thread.currentThread().interrupt()
        throw Unavailable()
    }
}
