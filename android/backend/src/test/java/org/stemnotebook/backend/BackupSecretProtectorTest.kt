package org.stemnotebook.backend

import org.junit.Assert.*
import org.junit.Test
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** Actual JCA AEAD with a test key provider; no Android Keystore/hardware claim. */
class BackupSecretProtectorTest {
    private class Keys : BackupSecretKeys {
        val values = java.util.concurrent.ConcurrentHashMap<String, SecretKey>()
        var encryptInput: ByteArray? = null
        var decryptOutput: ByteArray? = null
        var deleteAction: () -> Unit = {}
        var encryptAction: () -> Unit = {}
        override fun create(alias: String) {
            val generator = KeyGenerator.getInstance("AES"); generator.init(256)
            require(values.putIfAbsent(alias, generator.generateKey()) == null)
        }
        override fun encrypt(alias: String, plain: ByteArray, aad: ByteArray): BackupSecretKeys.Ciphertext {
            encryptInput = plain; encryptAction()
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.ENCRYPT_MODE, checkNotNull(values[alias])); cipher.updateAAD(aad)
            return BackupSecretKeys.Ciphertext(cipher.iv, cipher.doFinal(plain))
        }
        override fun decrypt(alias: String, value: BackupSecretKeys.Ciphertext, aad: ByteArray): ByteArray {
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, checkNotNull(values[alias]), GCMParameterSpec(128, value.nonce)); cipher.updateAAD(aad)
            return cipher.doFinal(value.bytes).also { decryptOutput = it }
        }
        override fun delete(alias: String) { deleteAction(); values.remove(alias) }
    }
    private fun context(purpose: BackupSecretContext.Purpose = BackupSecretContext.Purpose.SchedulePassword) = BackupSecretContext(UUID.randomUUID(), UUID.randomUUID(), purpose)
    private fun unavailable(action: () -> Unit) {
        try { action(); fail("Invalid secret operation accepted") }
        catch (expected: BackupSecretUnavailable) { assertEquals("Backup credential unavailable", expected.message); assertNull(expected.cause) }
    }
    @Test fun cipherRoundtripKeepsCallerOwnershipAndClearsTemporaryPlaintext() {
        val keys = Keys(); val protector = BackupSecretProtector(keys); val context = context()
        val input = "Distinct test backup passphrase 🖊".toCharArray(); val expected = input.clone()
        val sealed = protector.seal(context, input) { it.clone() }
        assertArrayEquals(expected, input)
        assertTrue(keys.encryptInput!!.all { it == 0.toByte() })
        assertFalse(sealed.toString(Charsets.UTF_8).contains("Distinct test"))
        var observed: CharArray? = null
        protector.use(context, sealed) { secret -> observed = secret; assertArrayEquals(expected, secret) }
        assertTrue(observed!!.all { it == '\u0000' }); assertTrue(keys.decryptOutput!!.all { it == 0.toByte() })
        input.fill('\u0000'); expected.fill('\u0000')
    }
    @Test fun tamperingAndContextSubstitutionNeverReleasePlaintext() {
        val keys = Keys(); val protector = BackupSecretProtector(keys); val context = context()
        val sealed = protector.seal(context, "Distinct passphrase".toCharArray()) { it.clone() }
        for (position in listOf(0, 8, 9, 21, sealed.lastIndex)) {
            val bad = sealed.clone(); bad[position] = (bad[position].toInt() xor 1).toByte()
            unavailable { protector.use(context, bad) { fail("Corrupt secret disclosed") } }
        }
        for (other in listOf(context.copy(id = UUID.randomUUID()), context.copy(library = UUID.randomUUID()), context.copy(purpose = BackupSecretContext.Purpose.RecoveryKey))) {
            // Give the other alias the same test key: AAD must still reject it.
            keys.values[other.alias] = keys.values.getValue(context.alias)
            unavailable { protector.use(other, sealed) { fail("Foreign context disclosed") } }
        }
    }
    @Test fun callbackFailuresClearPlaintextWithoutGuessingAStoreCommit() {
        val keys = Keys(); val protector = BackupSecretProtector(keys); val context = context()
        var sealed: ByteArray? = null
        unavailable { protector.seal(context, "Distinct passphrase".toCharArray()) { sealed = it.clone(); throw IllegalStateException("private callback detail") } }
        // A store may have committed before losing its acknowledgement. Keep key.
        assertNotNull(keys.values[context.alias]); assertTrue(keys.encryptInput!!.all { it == 0.toByte() })
        var observed: CharArray? = null
        unavailable { protector.use(context, sealed!!) { observed = it; throw IllegalStateException("private callback detail") } }
        assertTrue(observed!!.all { it == '\u0000' }); assertTrue(keys.decryptOutput!!.all { it == 0.toByte() })
    }
    @Test fun encryptionFailureAlsoClearsTheEncodedInput() {
        val keys = Keys(); keys.encryptAction = { throw IllegalStateException("injected platform failure") }
        val protector = BackupSecretProtector(keys)
        unavailable { protector.seal(context(), "Distinct passphrase".toCharArray()) { fail("Failed encryption committed") } }
        assertTrue(keys.encryptInput!!.all { it == 0.toByte() })
    }
    @Test fun passwordSizeAndRecoveryKeyDomainAreBoundedBeforeKeyCreation() {
        val keys = Keys(); val protector = BackupSecretProtector(keys)
        for (secret in listOf(charArrayOf(), CharArray(1025) { 'x' })) unavailable { protector.seal(context(), secret) { fail("Invalid size committed") } }
        val recovery = context(BackupSecretContext.Purpose.RecoveryKey)
        unavailable { protector.seal(recovery, CharArray(64) { 'Z' }) { fail("Invalid key committed") } }
        assertTrue(keys.values.isEmpty())
        val secret = CharArray(64) { "0123456789abcdef"[it % 16] }
        val sealed = protector.seal(recovery, secret) { it.clone() }
        protector.use(recovery, sealed) { assertArrayEquals(secret, it) }
        secret.fill('\u0000')
    }
    @Test fun oversizedTruncatedAndFutureEnvelopesAreRejected() {
        val protector = BackupSecretProtector(Keys()); val context = context()
        for (bytes in listOf(byteArrayOf(), ByteArray(38), ByteArray(2086))) unavailable { protector.use(context, bytes) { fail("Invalid envelope") } }
    }
    @Test fun consumerCancellationKeepsItsMeaningAndClearsProvidedBuffers() {
        val keys = Keys(); val protector = BackupSecretProtector(keys); val context = context()
        val sealed = protector.seal(context, "Distinct passphrase".toCharArray()) { it.clone() }
        var observed: CharArray? = null
        try {
            protector.use(context, sealed) { observed = it; throw java.util.concurrent.CancellationException("private cancellation detail") }
            fail("Cancellation ignored")
        } catch (expected: java.util.concurrent.CancellationException) { assertEquals("Backup credential operation cancelled", expected.message); assertNull(expected.cause) }
        assertTrue(observed!!.all { it == '\u0000' }); assertTrue(keys.decryptOutput!!.all { it == 0.toByte() })
    }
    @Test fun interruptionIsPreservedAfterStoreCallbackRetirement() {
        val keys = Keys(); val protector = BackupSecretProtector(keys)
        try {
            try { protector.seal(context(), "Distinct passphrase".toCharArray()) { throw InterruptedException("private interruption detail") }; fail("Interruption ignored") }
            catch (expected: java.util.concurrent.CancellationException) { assertEquals("Backup credential operation cancelled", expected.message) }
            assertTrue(Thread.currentThread().isInterrupted)
            assertTrue(keys.encryptInput!!.all { it == 0.toByte() })
        } finally { Thread.interrupted() }
    }
    @Test fun aliasesAreNotSilentlyOverwritten() {
        val keys = Keys(); val protector = BackupSecretProtector(keys); val context = context()
        val first = protector.seal(context, "First distinct passphrase".toCharArray()) { it.clone() }
        unavailable { protector.seal(context, "Different passphrase".toCharArray()) { fail("Alias overwritten") } }
        protector.use(context, first) { assertEquals("First distinct passphrase", String(it)) }
    }
    @Test(timeout = 15000) fun retirementWaitsForActualConsumerAndKeyDeletion() {
        val keys = Keys(); val protector = BackupSecretProtector(keys); val context = context()
        val sealed = protector.seal(context, "Distinct passphrase".toCharArray()) { it.clone() }
        val pool = Executors.newSingleThreadExecutor(); val entered = CountDownLatch(1); val release = CountDownLatch(1)
        val deleteEntered = CountDownLatch(1); val deleteRelease = CountDownLatch(1)
        keys.deleteAction = { deleteEntered.countDown(); check(deleteRelease.await(10, TimeUnit.SECONDS)) }
        try {
            val reading = pool.submit { protector.use(context, sealed) { entered.countDown(); check(release.await(10, TimeUnit.SECONDS)) } }
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            val retired = protector.retire(context)
            assertFalse(retired.isDone)
            unavailable { protector.use(context, sealed) { fail("Retiring key reopened") } }
            release.countDown(); assertTrue(deleteEntered.await(5, TimeUnit.SECONDS))
            assertFalse(retired.isDone); assertNotNull(keys.values[context.alias])
            deleteRelease.countDown(); retired.get(5, TimeUnit.SECONDS); reading.get(5, TimeUnit.SECONDS)
            assertNull(keys.values[context.alias])
            unavailable { protector.use(context, sealed) { fail("Deleted key opened") } }
        } finally { release.countDown(); deleteRelease.countDown(); pool.shutdownNow() }
    }
    @Test(timeout = 15000) fun keyLeaseIncludesTheCiphertextCommitCallback() {
        val keys = Keys(); val protector = BackupSecretProtector(keys); val context = context()
        val pool = Executors.newSingleThreadExecutor(); val entered = CountDownLatch(1); val release = CountDownLatch(1)
        try {
            val storing = pool.submit { protector.seal(context, "Distinct passphrase".toCharArray()) { entered.countDown(); check(release.await(10, TimeUnit.SECONDS)) } }
            assertTrue(entered.await(5, TimeUnit.SECONDS)); val retired = protector.retire(context)
            assertFalse(retired.isDone); assertNotNull(keys.values[context.alias])
            release.countDown(); storing.get(5, TimeUnit.SECONDS); retired.get(5, TimeUnit.SECONDS)
            assertNull(keys.values[context.alias])
        } finally { release.countDown(); pool.shutdownNow() }
    }
    @Test fun failedDeletionDeniesOpensAndCanBeExplicitlyRetried() {
        val keys = Keys(); val protector = BackupSecretProtector(keys); val context = context()
        val sealed = protector.seal(context, "Distinct passphrase".toCharArray()) { it.clone() }
        keys.deleteAction = { throw IllegalStateException("injected") }
        try { protector.retire(context).get(); fail("Deletion failure hidden") } catch (_: java.util.concurrent.ExecutionException) { }
        unavailable { protector.use(context, sealed) { fail("Failed retirement reopened") } }
        keys.deleteAction = {}; protector.retire(context).get()
        assertNull(keys.values[context.alias])
    }
    @Test(timeout = 15000) fun retirementContinuationsRunOutsideTheRegistryMonitor() {
        val keys = Keys(); val protector = BackupSecretProtector(keys); val context = context()
        val sealed = protector.seal(context, "Distinct passphrase".toCharArray()) { it.clone() }
        val consumer = Executors.newSingleThreadExecutor(); val callback = Executors.newSingleThreadExecutor()
        val entered = CountDownLatch(1); val release = CountDownLatch(1)
        try {
            val reading = consumer.submit { protector.use(context, sealed) { entered.countDown(); check(release.await(10, TimeUnit.SECONDS)) } }
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            val done = protector.retire(context).thenRun {
                callback.submit { protector.seal(context(), "Other distinct passphrase".toCharArray()) { } }.get(5, TimeUnit.SECONDS)
            }
            release.countDown(); done.get(10, TimeUnit.SECONDS); reading.get(10, TimeUnit.SECONDS)
        } finally { release.countDown(); consumer.shutdownNow(); callback.shutdownNow() }
    }
}
