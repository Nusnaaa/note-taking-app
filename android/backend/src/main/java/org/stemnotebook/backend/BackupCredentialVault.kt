package org.stemnotebook.backend

import java.util.UUID
import java.util.concurrent.CompletableFuture

/** Worker-only durable credential workflow. The app controller must supply fresh
 * consent/verification and hold library ownership through actual retirement. */
internal class BackupCredentialVault(private val store: BackupPolicyStore, private val protector: BackupSecretProtector) {
    fun store(context: BackupSecretContext, backup: UUID?, secret: CharArray, confirmedDeviceStorage: Boolean,
        independentRecoveryAcknowledged: Boolean) {
        store.beginCredential(context, backup, confirmedDeviceStorage, independentRecoveryAcknowledged)
        protector.seal(context, secret) { envelope -> store.commitCredential(context, envelope) }
    }
    fun <T> use(context: BackupSecretContext, action: (CharArray) -> T): T {
        val record = store.credential(context) ?: throw BackupPolicyStore.Unavailable()
        if (record.state != BackupPolicyStore.CredentialState.Ready) throw BackupPolicyStore.Unavailable()
        return protector.use(context, record.envelope!!) { secret ->
            // The durable check occurs inside the key lease. Retirement may have
            // started between reading the row and acquiring the lease.
            store.requireReady(context)
            action(secret)
        }
    }
    fun retire(context: BackupSecretContext): CompletableFuture<Void> {
        store.markRetired(context)
        return protector.retire(context).thenApply {
            // A failed/lost store acknowledgement leaves Retired/Deleted, never
            // Ready. Retrying after restart deletes idempotently before erasure.
            store.acknowledgeDeleted(context)
            null
        }
    }
}
