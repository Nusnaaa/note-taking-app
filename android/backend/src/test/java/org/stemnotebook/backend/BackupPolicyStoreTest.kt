package org.stemnotebook.backend

import android.database.sqlite.SQLiteDatabase
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.io.File
import java.io.IOException
import java.nio.file.Files
import java.nio.file.attribute.PosixFilePermissions
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class BackupPolicyStoreTest {
    @get:Rule val temporary = TemporaryFolder()
    private val library = UUID.randomUUID()
    private fun folder() = File(temporary.newFolder(), "policy")
    private fun context(recovery: Boolean = false) = BackupSecretContext(library, UUID.randomUUID(), if (recovery) BackupSecretContext.Purpose.RecoveryKey else BackupSecretContext.Purpose.SchedulePassword)
    private fun unavailable(action: () -> Unit) { try { action(); fail("Unavailable operation accepted") } catch (_: IOException) {} }
    private class Keys : BackupSecretKeys {
        val keys = mutableMapOf<String, SecretKey>()
        var failDelete = false
        var decryptCalls = 0
        var beforeCreate: () -> Unit = {}
        override fun create(alias: String) { beforeCreate(); check(!keys.containsKey(alias)); keys[alias] = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey() }
        override fun encrypt(alias: String, plain: ByteArray, aad: ByteArray): BackupSecretKeys.Ciphertext {
            val cipher = Cipher.getInstance("AES/GCM/NoPadding"); cipher.init(Cipher.ENCRYPT_MODE, keys.getValue(alias)); cipher.updateAAD(aad)
            return BackupSecretKeys.Ciphertext(cipher.iv, cipher.doFinal(plain))
        }
        override fun decrypt(alias: String, value: BackupSecretKeys.Ciphertext, aad: ByteArray): ByteArray {
            decryptCalls++
            val cipher = Cipher.getInstance("AES/GCM/NoPadding"); cipher.init(Cipher.DECRYPT_MODE, keys.getValue(alias), GCMParameterSpec(128, value.nonce)); cipher.updateAAD(aad)
            return cipher.doFinal(value.bytes)
        }
        override fun delete(alias: String) { if (failDelete) throw IOException("private provider failure"); keys.remove(alias) }
    }
    @Test fun actualEncryptedCredentialSurvivesStoreAndControllerRestartWithoutPlaintextRows() {
        val folder = folder(); val keys = Keys(); val context = context(); val secret = "portable passphrase Ä∫🐈".toCharArray()
        val store = BackupPolicyStore(folder)
        keys.beforeCreate = { assertEquals(BackupPolicyStore.CredentialState.Intent, BackupPolicyStore(folder).credential(context)!!.state) }
        BackupCredentialVault(store, BackupSecretProtector(keys)).store(context, null, secret, true, true)
        var observed: CharArray? = null
        BackupCredentialVault(BackupPolicyStore(folder), BackupSecretProtector(keys)).use(context) { observed = it; assertArrayEquals(secret, it) }
        assertTrue(observed!!.all { it == '\u0000' }); assertTrue(secret.any { it != '\u0000' })
        assertFalse(File(folder, "policy.db").readBytes().toString(Charsets.ISO_8859_1).contains("portable passphrase"))
        assertEquals(BackupPolicyStore.CredentialState.Ready, BackupPolicyStore(folder).credential(context)!!.state)
    }
    @Test fun missingConsentCannotCreateIntentOrKey() {
        val folder = folder(); val keys = Keys(); val store = BackupPolicyStore(folder); val vault = BackupCredentialVault(store, BackupSecretProtector(keys))
        for ((consent, recovery) in listOf(false to true, true to false)) {
            val context = context(); unavailable { vault.store(context, null, "credential".toCharArray(), consent, recovery) }
            assertNull(store.credential(context)); assertTrue(keys.keys.isEmpty())
        }
    }
    @Test fun lostIntentAcknowledgementNeverCreatesKeyAndRemainsDiscoverableAfterRestart() {
        val folder = folder(); val context = context(); val keys = Keys()
        val failing = BackupPolicyStore(folder) { if (it == "policy-after-commit") throw IOException("lost") }
        unavailable { BackupCredentialVault(failing, BackupSecretProtector(keys)).store(context, null, "password".toCharArray(), true, true) }
        assertTrue(keys.keys.isEmpty())
        val restarted = BackupPolicyStore(folder)
        assertEquals(context, restarted.credentials(library).single().context)
        assertEquals(BackupPolicyStore.CredentialState.Intent, restarted.credential(context)!!.state)
        unavailable { BackupCredentialVault(restarted, BackupSecretProtector(keys)).use(context) {} }
    }
    @Test fun lostEncryptedCommitAcknowledgementKeepsDecryptableReadyRecordAndKey() {
        val folder = folder(); val context = context(); val keys = Keys(); var commits = 0
        val failing = BackupPolicyStore(folder) { if (it == "policy-after-commit" && ++commits == 2) throw IOException("lost") }
        val secret = "password with separate recovery".toCharArray()
        try { BackupCredentialVault(failing, BackupSecretProtector(keys)).store(context, null, secret, true, true); fail() } catch (_: IOException) {}
        assertTrue(keys.keys.containsKey(context.alias))
        BackupCredentialVault(BackupPolicyStore(folder), BackupSecretProtector(keys)).use(context) { assertArrayEquals(secret, it) }
    }
    @Test fun rollbackBeforeEncryptedCommitLeavesAnIntentAndNeverReady() {
        val folder = folder(); val context = context(); val keys = Keys(); var commits = 0
        val failing = BackupPolicyStore(folder) { if (it == "policy-before-commit" && ++commits == 2) throw IOException("injected") }
        unavailable { BackupCredentialVault(failing, BackupSecretProtector(keys)).store(context, null, "credential".toCharArray(), true, true) }
        val restarted = BackupPolicyStore(folder)
        assertEquals(BackupPolicyStore.CredentialState.Intent, restarted.credential(context)!!.state)
        unavailable { BackupCredentialVault(restarted, BackupSecretProtector(keys)).use(context) {} }
        BackupCredentialVault(restarted, BackupSecretProtector(keys)).retire(context).join()
        assertTrue(keys.keys.isEmpty()); assertNull(restarted.credential(context)!!.envelope)
    }
    @Test fun retirementWaitsForActualConsumerAndRestartRetriesFailedKeyDeletion() {
        val folder = folder(); val store = BackupPolicyStore(folder); val keys = Keys(); val context = context()
        val vault = BackupCredentialVault(store, BackupSecretProtector(keys)); vault.store(context, null, "credential".toCharArray(), true, true)
        val entered = CountDownLatch(1); val release = CountDownLatch(1); var clear: CharArray? = null
        val consumerFailure = java.util.concurrent.atomic.AtomicReference<Throwable?>()
        val thread = Thread { try { vault.use(context) { clear = it; entered.countDown(); check(release.await(30, TimeUnit.SECONDS)) } } catch (error: Throwable) { consumerFailure.set(error) } }
        thread.start(); assertTrue(entered.await(10, TimeUnit.SECONDS)); keys.failDelete = true
        val completion = vault.retire(context); assertFalse(completion.isDone)
        assertEquals(BackupPolicyStore.CredentialState.Retired, BackupPolicyStore(folder).credential(context)!!.state)
        unavailable { vault.use(context) {} }
        release.countDown(); thread.join(30000); assertFalse(thread.isAlive); assertNull(consumerFailure.get())
        try { completion.join(); fail() } catch (_: java.util.concurrent.CompletionException) {}
        assertTrue(clear!!.all { it == '\u0000' }); assertNotNull(store.credential(context)!!.envelope)
        keys.failDelete = false
        BackupCredentialVault(BackupPolicyStore(folder), BackupSecretProtector(keys)).retire(context).join()
        assertEquals(BackupPolicyStore.CredentialState.Deleted, store.credential(context)!!.state)
        assertNull(store.credential(context)!!.envelope); assertTrue(keys.keys.isEmpty())
        unavailable { vault.store(context, null, "another".toCharArray(), true, true) }
    }
    @Test fun recoveryKeysHaveBackupIdentityAndCannotCrossPurposeOrLibrary() {
        val folder = folder(); val store = BackupPolicyStore(folder); val keys = Keys(); val vault = BackupCredentialVault(store, BackupSecretProtector(keys))
        val context = context(true); val backup = UUID.randomUUID()
        unavailable { vault.store(context, null, "a".repeat(64).toCharArray(), true, true) }
        vault.store(context, backup, "a".repeat(64).toCharArray(), true, true)
        assertEquals(backup, BackupPolicyStore(folder).credential(context)!!.backup)
        unavailable { store.credential(context.copy(library = UUID.randomUUID())) }
        unavailable { store.credential(context.copy(purpose = BackupSecretContext.Purpose.SchedulePassword)) }
        unavailable { vault.store(context(), backup, "credential".toCharArray(), true, true) }
    }
    private fun decision() = BackupPersistentAccess.Decision(UUID.randomUUID(), library, "content://example/tree/directory", "directory", true, false, 123)
    @Test fun providerConsentDecisionAndUncertainGrantSurviveRestartWithoutInventingSuccess() {
        val folder = folder(); val store = BackupPolicyStore(folder); val decision = decision()
        store.recordPermissionDecision(decision)
        assertNull(BackupPolicyStore(folder).permission(decision.id)!!.second)
        val receipt = BackupPersistentAccess.Receipt(decision, BackupPersistentAccess.Outcome.Uncertain, null, null)
        store.recordPermissionOutcome(receipt)
        assertEquals(decision to receipt, BackupPolicyStore(folder).permission(decision.id))
        unavailable { store.recordPermissionDecision(decision) }
        unavailable { store.recordPermissionOutcome(receipt.copy(outcome = BackupPersistentAccess.Outcome.Persisted, observedRead = true, observedWrite = true)) }
    }
    @Test fun outcomesRequireOriginalDecisionAndCompleteFlagsForKnownSuccess() {
        val store = BackupPolicyStore(folder()); val decision = decision(); store.recordPermissionDecision(decision)
        val receipt = BackupPersistentAccess.Receipt(decision, BackupPersistentAccess.Outcome.Persisted, true, true)
        unavailable { store.recordPermissionOutcome(receipt.copy(decision = decision.copy(library = UUID.randomUUID()))) }
        unavailable { store.recordPermissionOutcome(receipt.copy(observedWrite = false)) }
        assertNull(store.permission(decision.id)!!.second)
        store.recordPermissionOutcome(receipt); assertEquals(receipt, store.permission(decision.id)!!.second)
    }
    @Test fun boundedPagesDiscoverIntentAndPermissionDecisionsWithoutMixingLibraries() {
        val folder = folder(); val store = BackupPolicyStore(folder)
        repeat(7) { store.beginCredential(context(), null, true, true); store.recordPermissionDecision(decision()) }
        store.beginCredential(context().copy(library = UUID.randomUUID()), null, true, true)
        val first = store.credentials(library, limit = 3)
        val rest = store.credentials(library, first.last().context.id, 100)
        assertEquals(7, (first + rest).map { it.context.id }.toSet().size)
        val permissions = store.permissions(library, limit = 3)
        val remaining = store.permissions(library, permissions.last().first.id, 100)
        assertEquals(7, (permissions + remaining).map { it.first.id }.toSet().size)
        assertTrue((permissions + remaining).all { it.second == null })
        unavailable { store.credentials(library, limit = 101) }
        unavailable { store.permissions(library, limit = 0) }
    }
    @Test fun malformedOversizedAndTamperedEnvelopesNeverReachConsumer() {
        val folder = folder(); val store = BackupPolicyStore(folder); val context = context(); val keys = Keys()
        val vault = BackupCredentialVault(store, BackupSecretProtector(keys))
        vault.store(context, null, "credential".toCharArray(), true, true)
        val original = store.credential(context)!!.envelope!!
        for (value in listOf(byteArrayOf(1), ByteArray(2090), original.copyOf().apply { this[lastIndex] = (this[lastIndex].toInt() xor 1).toByte() })) {
            SQLiteDatabase.openDatabase(File(folder, "policy.db").path, null, SQLiteDatabase.NO_LOCALIZED_COLLATORS).use { db ->
                db.execSQL("UPDATE credentials SET envelope=? WHERE id=?", arrayOf(value, context.id.toString()))
            }
            var called = false
            keys.decryptCalls = 0
            unavailable { vault.use(context) { called = true } }; assertFalse(called)
            assertEquals(if (value.size == original.size) 1 else 0, keys.decryptCalls)
        }
        SQLiteDatabase.openDatabase(File(folder, "policy.db").path, null, SQLiteDatabase.NO_LOCALIZED_COLLATORS).use { db ->
            db.execSQL("UPDATE credentials SET envelope=zeroblob(4000000) WHERE id=?", arrayOf(context.id.toString()))
        }
        unavailable { store.credential(context) }
    }
    @Test fun deletionAcknowledgementLossCannotReviveCredentialAndRetryIsIdempotent() {
        val folder = folder(); val context = context(); val keys = Keys(); val store = BackupPolicyStore(folder)
        BackupCredentialVault(store, BackupSecretProtector(keys)).store(context, null, "credential".toCharArray(), true, true)
        var commits = 0
        val failing = BackupPolicyStore(folder) { if (it == "policy-after-commit" && ++commits == 2) throw IOException("lost delete receipt") }
        try { BackupCredentialVault(failing, BackupSecretProtector(keys)).retire(context).join(); fail() } catch (_: java.util.concurrent.CompletionException) {}
        assertEquals(BackupPolicyStore.CredentialState.Deleted, store.credential(context)!!.state)
        assertNull(store.credential(context)!!.envelope); assertTrue(keys.keys.isEmpty())
        BackupCredentialVault(store, BackupSecretProtector(keys)).retire(context).join()
        unavailable { BackupCredentialVault(store, BackupSecretProtector(keys)).use(context) {} }
    }
    @Test fun interruptedCommitPreservesThreadFlagAndDiscoverablePossibleEffect() {
        val folder = folder(); val context = context()
        val store = BackupPolicyStore(folder) { if (it == "policy-after-commit") throw InterruptedException("private") }
        try {
            try { store.beginCredential(context, null, true, true); fail() } catch (_: java.util.concurrent.CancellationException) {}
            assertTrue(Thread.currentThread().isInterrupted)
        } finally { Thread.interrupted() }
        assertEquals(BackupPolicyStore.CredentialState.Intent, BackupPolicyStore(folder).credential(context)!!.state)
    }
    @Test fun foreignFutureAndUnexpectedSchemasArePreserved() {
        for (mode in 0..2) {
            val folder = folder()
            if (mode == 2) {
                BackupPolicyStore(folder).credentials(library)
                SQLiteDatabase.openDatabase(File(folder, "policy.db").path, null, SQLiteDatabase.NO_LOCALIZED_COLLATORS).use { it.execSQL("CREATE TABLE unexpected(value TEXT)") }
            } else {
                Files.createDirectory(folder.toPath(), PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rwx------")))
                SQLiteDatabase.openOrCreateDatabase(File(folder, "policy.db"), null).use { it.execSQL("CREATE TABLE original(value TEXT)"); it.version = mode + 1 }
                Files.setPosixFilePermissions(File(folder, "policy.db").toPath(), PosixFilePermissions.fromString("rw-------"))
            }
            val before = File(folder, "policy.db").readBytes()
            unavailable { BackupPolicyStore(folder).credentials(library) }
            assertArrayEquals(before, File(folder, "policy.db").readBytes())
        }
    }
    @Test fun linkedFilesUnknownNamespaceAndAnchoredReplacementAreRejected() {
        for (hard in listOf(false, true)) {
            val folder = folder(); Files.createDirectory(folder.toPath(), PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rwx------")))
            val original = temporary.newFile().apply { writeText("preserve") }
            Files.setPosixFilePermissions(original.toPath(), PosixFilePermissions.fromString("rw-------"))
            if (hard) Files.createLink(File(folder, "policy.db").toPath(), original.toPath()) else Files.createSymbolicLink(File(folder, "policy.db").toPath(), original.toPath())
            unavailable { BackupPolicyStore(folder).credentials(library) }; assertEquals("preserve", original.readText())
        }
        val folder = folder(); val store = BackupPolicyStore(folder); store.credentials(library)
        val relocated = File(folder.parentFile, "old"); assertTrue(folder.renameTo(relocated))
        unavailable { store.credentials(library) }; assertFalse(folder.exists())
        val other = folder(); Files.createDirectory(other.toPath(), PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rwx------")))
        File(other, "unexpected").writeText("keep")
        unavailable { BackupPolicyStore(other).credentials(library) }; assertFalse(File(other, "policy.db").exists())
    }
}
