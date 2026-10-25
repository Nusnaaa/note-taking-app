package org.stemnotebook.backend

import android.app.KeyguardManager
import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.io.IOException
import java.security.KeyStore
import java.util.UUID
import java.util.concurrent.CompletableFuture
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** Device-local purpose separation; these references never grant notebook access. */
internal data class BackupSecretContext(val library: UUID, val id: UUID, val purpose: Purpose) {
    enum class Purpose { SchedulePassword, RecoveryKey }
    val alias get() = "stem.backup.${purpose.name}.${library}.${id}"
    fun aad(): ByteArray = "STEM-BACKUP-SECRET-1:${purpose.name}:$library:$id".toByteArray(Charsets.US_ASCII)
}
internal class BackupSecretUnavailable : IOException("Backup credential unavailable")

/** Implementations must not export their key or replace an existing alias. All
 * operations belong on the optional worker, never the input/writer/main thread. */
internal interface BackupSecretKeys {
    data class Ciphertext(val nonce: ByteArray, val bytes: ByteArray)
    fun create(alias: String)
    fun encrypt(alias: String, plain: ByteArray, aad: ByteArray): Ciphertext
    fun decrypt(alias: String, value: Ciphertext, aad: ByteArray): ByteArray
    fun delete(alias: String)
}

/** Android Keystore adapter. No software fallback and no key-export claim.
 * Unlocked-device key limitations on Android 12-14 may make operations fail;
 * callers must retain the independent backup passphrase/recovery path. */
internal class AndroidBackupSecretKeys(context: Context) : BackupSecretKeys {
    private val keyguard = context.getSystemService(KeyguardManager::class.java)
    private fun unlocked() { if (keyguard == null || !keyguard.isDeviceSecure || keyguard.isDeviceLocked) throw BackupSecretUnavailable() }
    private fun store(): KeyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
    private fun key(alias: String): SecretKey = store().getKey(alias, null) as? SecretKey ?: throw BackupSecretUnavailable()
    @Synchronized override fun create(alias: String) {
        unlocked()
        require(!store().containsAlias(alias))
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        generator.init(KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
            .setKeySize(256).setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setRandomizedEncryptionRequired(true).setUnlockedDeviceRequired(true)
            .setUserAuthenticationRequired(false).build())
        generator.generateKey()
        unlocked()
    }
    @Synchronized override fun encrypt(alias: String, plain: ByteArray, aad: ByteArray): BackupSecretKeys.Ciphertext {
        unlocked()
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key(alias)); cipher.updateAAD(aad)
        val encrypted = cipher.doFinal(plain)
        unlocked()
        return BackupSecretKeys.Ciphertext(cipher.iv, encrypted)
    }
    @Synchronized override fun decrypt(alias: String, value: BackupSecretKeys.Ciphertext, aad: ByteArray): ByteArray {
        unlocked()
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key(alias), GCMParameterSpec(128, value.nonce)); cipher.updateAAD(aad)
        val plain = cipher.doFinal(value.bytes)
        return try { unlocked(); plain } catch (error: Exception) { plain.fill(0); throw error }
    }
    @Synchronized override fun delete(alias: String) { store().deleteEntry(alias) }
}

/** Cipher/lease foundation only. A persisted scheduling consent and store must
 * gate callers; this primitive does not itself enable or register a schedule.
 * Password quality is checked by the native initial-backup workflow, not guessed
 * here. Caller owns the original input and any explicitly made copies. */
internal class BackupSecretProtector(private val keys: BackupSecretKeys) {
    private class Entry {
        var users = 0
        var retiring = false
        var deleting = false
        var failed = false
        var completion = CompletableFuture<Void>()
    }
    private val lock = Any()
    private val entries = mutableMapOf<BackupSecretContext, Entry>()
    private val magic = "STEMKEY1".toByteArray(Charsets.US_ASCII)
    private fun acquire(context: BackupSecretContext): Entry = synchronized(lock) {
        val entry = entries.getOrPut(context) { Entry() }
        if (entry.retiring) throw BackupSecretUnavailable()
        entry.users++; entry
    }
    private fun release(context: BackupSecretContext, entry: Entry) {
        val deletion = synchronized(lock) {
            check(entry.users > 0); entry.users--
            if (entry.users == 0 && entry.retiring && !entry.deleting) { entry.deleting = true; true }
            else { if (entry.users == 0 && !entry.retiring) entries.remove(context); false }
        }
        if (deletion) deleteKey(context, entry)
    }
    private fun deleteKey(context: BackupSecretContext, entry: Entry) {
        val error = try { keys.delete(context.alias); null } catch (_: Exception) { BackupSecretUnavailable() }
        val completion = synchronized(lock) {
            entry.deleting = false
            if (error == null) entries.remove(context) else entry.failed = true
            entry.completion
        }
        // Future continuations may call trusted code or wait on another worker.
        // Never invoke them under the registry monitor.
        if (error == null) completion.complete(null) else completion.completeExceptionally(error)
    }
    /** Completes only after actual in-use callbacks retire and key deletion ends.
     * Storage must mark the reference retired before invoking this and retain its
     * ciphertext on an uncertain/failed delete. New opens are denied meanwhile. */
    fun retire(context: BackupSecretContext): CompletableFuture<Void> {
        val entry: Entry
        val deletion: Boolean
        synchronized(lock) {
            entry = entries.getOrPut(context) { Entry() }
            if (entry.failed && !entry.deleting) { entry.completion = CompletableFuture(); entry.failed = false }
            entry.retiring = true
            deletion = entry.users == 0 && !entry.deleting
            if (deletion) entry.deleting = true
        }
        if (deletion) deleteKey(context, entry)
        return entry.completion
    }
    /** The encrypted-store callback is part of the key lease. A cancellation or
     * failure after its commit must preserve the encrypted record/key for review;
     * this primitive never guesses whether an external commit happened. */
    fun <T> seal(context: BackupSecretContext, secret: CharArray, commit: (ByteArray) -> T): T {
        if (secret.isEmpty() || secret.size > 1024) throw BackupSecretUnavailable()
        if (context.purpose == BackupSecretContext.Purpose.RecoveryKey && (secret.size != 64 || secret.any { it !in '0'..'9' && it !in 'a'..'f' && it !in 'A'..'F' })) throw BackupSecretUnavailable()
        val entry = acquire(context)
        var plain: ByteArray? = null
        try {
            val encoded = ByteArray(secret.size * 2)
            plain = encoded
            secret.forEachIndexed { index, c -> encoded[index * 2] = (c.code ushr 8).toByte(); encoded[index * 2 + 1] = c.code.toByte() }
            keys.create(context.alias)
            val value = try { keys.encrypt(context.alias, encoded, context.aad()) } finally { encoded.fill(0) }
            if (value.nonce.size != 12 || value.bytes.size != secret.size * 2 + 16) throw BackupSecretUnavailable()
            val envelope = magic + byteArrayOf(1) + value.nonce + value.bytes
            return commit(envelope)
        } catch (error: Exception) { throw sanitized(error) }
        finally { plain?.fill(0); release(context, entry) }
    }
    private fun sanitized(error: Exception): Exception {
        if (error is InterruptedException) Thread.currentThread().interrupt()
        return if (error is InterruptedException || error is java.util.concurrent.CancellationException)
            java.util.concurrent.CancellationException("Backup credential operation cancelled") else BackupSecretUnavailable()
    }
    fun <T> use(context: BackupSecretContext, envelope: ByteArray, action: (CharArray) -> T): T {
        if (envelope.size !in 39..2085 || !envelope.copyOfRange(0, 8).contentEquals(magic) || envelope[8] != 1.toByte()) throw BackupSecretUnavailable()
        val entry = acquire(context)
        var plain: ByteArray? = null
        var secret: CharArray? = null
        try {
            plain = keys.decrypt(context.alias, BackupSecretKeys.Ciphertext(envelope.copyOfRange(9, 21), envelope.copyOfRange(21, envelope.size)), context.aad())
            if (plain.isEmpty() || plain.size % 2 != 0 || plain.size > 2048) throw BackupSecretUnavailable()
            secret = CharArray(plain.size / 2) { index -> (((plain[index * 2].toInt() and 255) shl 8) or (plain[index * 2 + 1].toInt() and 255)).toChar() }
            if (context.purpose == BackupSecretContext.Purpose.RecoveryKey && (secret.size != 64 || secret.any { it !in '0'..'9' && it !in 'a'..'f' && it !in 'A'..'F' })) throw BackupSecretUnavailable()
            plain.fill(0)
            return action(secret)
        } catch (error: Exception) { throw sanitized(error) }
        finally { secret?.fill('\u0000'); plain?.fill(0); release(context, entry) }
    }
}
