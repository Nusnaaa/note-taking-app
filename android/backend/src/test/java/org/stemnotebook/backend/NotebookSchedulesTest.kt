package org.stemnotebook.backend

import android.content.ContextWrapper
import android.content.Intent
import android.net.Uri
import android.os.CancellationSignal
import android.os.ParcelFileDescriptor
import android.provider.DocumentsContract
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import java.io.File
import java.io.IOException
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** Actual native encryption/publication, JCA test keys and synthetic destination
 * permissions. No actual Android Keystore/provider/device guarantee is inferred. */
@RunWith(JniBackendRunner::class)
@Config(sdk = [35])
class NotebookSchedulesTest {
    @get:Rule val temporary = TemporaryFolder()
    private val password get() = "Scheduled portable backup passphrase ∫ 2943".toCharArray()
    private val proof = NativeBridge.Verification { true }
    private class Keys : BackupSecretKeys {
        val keys = ConcurrentHashMap<String, SecretKey>()
        var decrypts = 0
        var failCreate = false
        override fun create(alias: String) { if (failCreate) throw IOException("synthetic device-key failure"); check(!keys.containsKey(alias)); keys[alias] = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey() }
        override fun encrypt(alias: String, plain: ByteArray, aad: ByteArray): BackupSecretKeys.Ciphertext {
            val cipher = Cipher.getInstance("AES/GCM/NoPadding"); cipher.init(Cipher.ENCRYPT_MODE, keys.getValue(alias)); cipher.updateAAD(aad)
            return BackupSecretKeys.Ciphertext(cipher.iv, cipher.doFinal(plain))
        }
        override fun decrypt(alias: String, value: BackupSecretKeys.Ciphertext, aad: ByteArray): ByteArray {
            decrypts++
            val cipher = Cipher.getInstance("AES/GCM/NoPadding"); cipher.init(Cipher.DECRYPT_MODE, keys.getValue(alias), GCMParameterSpec(128, value.nonce)); cipher.updateAAD(aad)
            return cipher.doFinal(value.bytes)
        }
        override fun delete(alias: String) { keys.remove(alias) }
    }
    private class Jobs : BackupJobScheduler() {
        var token: Token? = null
        var result = Registration.Registered
        var registrations = 0
        override fun minimumPeriodMillis() = 1000L // Synthetic adapter admission, not a real OS interval.
        override fun register(configuration: BackupSchedulePolicy.Configuration): Registration {
            registrations++
            if (result != Registration.Rejected) token = Token(configuration.id, configuration.revision)
            return result
        }
        override fun pending() = token
        override fun cancel(token: Token): Cancellation = when {
            this.token == null -> Cancellation.Absent
            this.token != token -> Cancellation.Conflict
            else -> { this.token = null; Cancellation.Cancelled }
        }
    }
    private class Permissions : BackupPersistentPermissions() {
        var held = Held(false, false); var takes = 0; var afterTake: () -> Unit = {}
        override fun current(tree: Uri) = held
        override fun take(tree: Uri) { takes++; held = Held(true, true); afterTake() }
    }
    private inner class Documents(private val directory: File) : BackupDocuments() {
        var available = true
        val files = mutableMapOf<Uri, File>()
        override fun inspect(tree: Uri, signal: CancellationSignal): Selection {
            signal.throwIfCanceled(); check(available); return Selection("root", "Reviewed backups")
        }
        override fun create(tree: Uri, name: String): Uri = DocumentsContract.buildDocumentUriUsingTree(tree, UUID.randomUUID().toString()).also {
            files[it] = File(directory, name).apply { check(createNewFile()) }
        }
        override fun isChild(tree: Uri, document: Uri) = files.containsKey(document)
        override fun open(document: Uri, write: Boolean, signal: CancellationSignal): ParcelFileDescriptor {
            signal.throwIfCanceled()
            return ParcelFileDescriptor.open(files.getValue(document), if (write) ParcelFileDescriptor.MODE_WRITE_ONLY else ParcelFileDescriptor.MODE_READ_ONLY)
        }
    }
    private fun context(root: File) = object : ContextWrapper(RuntimeEnvironment.getApplication()) { override fun getNoBackupFilesDir() = root }
    private fun backend(root: File, jobs: BackupJobScheduler, keys: Keys, permissions: Permissions = Permissions(), documents: BackupDocuments = BackupDocuments.platform(context(root))) =
        NotebookBackend(context(root), pdfFactory = { platform, native -> PdfWorkspace(platform, native) }, backupDocuments = documents,
            scheduleKeys = keys, scheduleJobs = jobs, schedulePermissions = permissions)
    private fun create(backend: NotebookBackend): UUID {
        val response = JSONObject(backend.request("""{"version":1,"command":{"op":"Create","kind":"Note","parent":null,"title":"scheduled source"}}""").get(10, TimeUnit.SECONDS))
        assertTrue(response.toString(), response.getBoolean("ok"))
        return UUID.fromString(response.getJSONObject("data").getString("id"))
    }
    private fun prepare(backend: NotebookBackend, target: NotebookSchedules.Target): NotebookSchedules.Prepared {
        val input = password
        return try { backend.schedules().prepare(NotebookSchedules.Options(target, 1000, 10_000_000), input, proof).await() }
        finally { input.fill('\u0000') }
    }
    private fun offered(stage: NotebookSchedules.Prepared) { val key = stage.copyRecoveryKey(); try { assertEquals(64, key.size) } finally { key.fill('\u0000') } }
    private fun allKeys(controller: NotebookSchedules): List<NotebookSchedules.StoredKey> {
        var after: UUID? = null; val found = mutableListOf<NotebookSchedules.StoredKey>()
        do { val page = controller.storedRecoveryKeys(after, 1).await(); found.addAll(page.keys); after = page.next } while (after != null)
        return found
    }
    @Test fun explicitEnableDueBackupRecoveryRevealAndDisableOperateOnActualEncryptedFiles() {
        val root = temporary.newFolder(); val destination = temporary.newFolder(); val jobs = Jobs(); val keys = Keys(); val backend = backend(root, jobs, keys)
        try {
            create(backend); val schedules = backend.schedules(); val stage = prepare(backend, NotebookSchedules.Target.Directory(destination)); offered(stage)
            val enabled = schedules.enable(stage, proof, true, true, false, true).await()
            assertFalse(enabled.needsAttention); assertEquals(NotebookSchedules.State.Enabled, enabled.configuration!!.state)
            assertEquals(NotebookSchedules.Publication.Published, enabled.initialPublication); assertTrue(enabled.catalogueRecorded)
            assertTrue(File(destination, "backup-${enabled.backup}.stemenc").length() > 0)
            assertEquals(enabled.backup.toString(), backend.backups().history().await().single().id)
            val token = jobs.token!!; val due = schedules.runDue(token).await()
            assertEquals("Published", due.result); assertNotNull(due.run); assertNotEquals(enabled.backup, due.backup)
            assertEquals(2, destination.listFiles()!!.size)
            val refs = allKeys(schedules); assertEquals(2, refs.size)
            val nextKey = refs.single { it.backup == due.backup }
            val secret = schedules.revealRecoveryKey(nextKey.id, proof, true).await()
            try { assertEquals(64, secret.size) } finally { secret.fill('\u0000') }
            assertTrue(schedules.deleteRecoveryKey(nextKey.id, proof, true).await().acknowledged)
            assertThrows(NotebookSchedules.Unavailable::class.java) { schedules.revealRecoveryKey(nextKey.id, proof, true).await() }
            val disabled = schedules.disable(enabled.configuration.revision, proof, true).await()
            assertEquals(NotebookSchedules.State.Disabled, disabled.configuration.state)
            assertTrue(disabled.credentialDeletionAcknowledged); assertTrue(disabled.jobCancellationAcknowledged); assertTrue(disabled.workRetirementAcknowledged)
            assertEquals("Ignored", schedules.runDue(token).await().result); assertEquals(2, destination.listFiles()!!.size)
            assertEquals(1, keys.keys.size) // Retained independent first-backup key only.
        } finally { backend.closeFullyAsync().get(20, TimeUnit.SECONDS) }
    }
    @Test fun reviewedDisabledRouteRetirementPreservesPolicyKeysHistoryAndBackupFiles() {
        val root = temporary.newFolder(); val destination = temporary.newFolder(); val keys = Keys()
        val platformJobs = BackupJobScheduler.platform(context(root)); val backend = backend(root, platformJobs, keys)
        try {
            create(backend)
            val input = password
            val stage = try { backend.schedules().prepare(NotebookSchedules.Options(NotebookSchedules.Target.Directory(destination), platformJobs.minimumPeriodMillis(), 10000000), input, proof).await() }
                finally { input.fill('\u0000') }
            offered(stage)
            val enabled = backend.schedules().enable(stage, proof, true, true, false, true).await()
            val enabledRevision = enabled.configuration!!.revision
            assertEquals(NotebookSchedules.State.Enabled, enabled.configuration.state)
            assertThrows(NotebookSchedules.Unavailable::class.java) { backend.schedules().retireDisabledRoute(enabledRevision, proof, true).await() }
            val disabled = backend.schedules().disable(enabledRevision, proof, true).await().configuration
            val routes = BackupJobRoutes(RuntimeEnvironment.getApplication().noBackupFilesDir)
            assertNotNull(routes.root(root))
            assertThrows(NotebookSchedules.Unavailable::class.java) { backend.schedules().retireDisabledRoute(disabled.revision, NativeBridge.Verification { false }, true).await() }
            assertThrows(NotebookSchedules.Unavailable::class.java) { backend.schedules().retireDisabledRoute(disabled.revision, proof, false).await() }
            assertNotNull(routes.root(root))
            val credentials = backend.schedules().storedCredentials().await().credentials
            val history = backend.backups().history().await()
            val receipt = backend.schedules().retireDisabledRoute(disabled.revision, proof, true).await()
            assertEquals(NotebookSchedules.RouteRetirement.Released, receipt.outcome)
            assertNull(routes.root(root)); assertNull(platformJobs.pending())
            assertEquals(disabled, backend.schedules().status().await().configuration)
            assertEquals(credentials, backend.schedules().storedCredentials().await().credentials)
            assertEquals(history, backend.backups().history().await())
            assertTrue(File(destination, "backup-${enabled.backup}.stemenc").isFile)
            assertEquals(NotebookSchedules.RouteRetirement.Absent, backend.schedules().retireDisabledRoute(disabled.revision, proof, true).await().outcome)
        } finally { backend.closeFullyAsync().get(20, TimeUnit.SECONDS) }
    }
    @Test fun sameEncryptedArtifactCanPublishDistinctAcknowledgedCopiesWithoutCatalogueIdCollision() {
        val root = temporary.newFolder(); val destination = temporary.newFolder(); val backend = backend(root, Jobs(), Keys())
        try {
            create(backend); val stage = prepare(backend, NotebookSchedules.Target.Directory(destination))
            try {
                val first = stage.artifact.publish(File(destination, "first.stemenc"), true).await()
                val second = stage.artifact.publish(File(destination, "second.stemenc"), true).await()
                assertTrue(first.catalogueRecorded); assertTrue(second.catalogueRecorded)
                val history = backend.backups().history().await()
                assertEquals(2, history.size); assertEquals(2, history.map { it.id }.toSet().size)
                assertArrayEquals(File(destination, "first.stemenc").readBytes(), File(destination, "second.stemenc").readBytes())
            } finally { stage.close(); backend.backups().awaitRetired() }
        } finally { backend.closeFullyAsync().get(20, TimeUnit.SECONDS) }
    }
    @Test fun failedDeviceCredentialSetupKeepsKnownBackupAndAllowsVerifiedUnusedIntentRetirement() {
        val root = temporary.newFolder(); val destination = temporary.newFolder(); val jobs = Jobs(); val keys = Keys().apply { failCreate = true }; val backend = backend(root, jobs, keys)
        try {
            create(backend); val stage = prepare(backend, NotebookSchedules.Target.Directory(destination)); offered(stage)
            val receipt = backend.schedules().enable(stage, proof, true, true, false, true).await()
            assertEquals(NotebookSchedules.Publication.Published, receipt.initialPublication); assertTrue(receipt.needsAttention)
            assertNull(receipt.configuration); assertNotNull(receipt.credential); assertEquals(0, jobs.registrations)
            assertEquals(1, destination.listFiles()!!.size)
            val stored = backend.schedules().storedCredentials().await().credentials.single()
            assertEquals(receipt.credential, stored.id); assertEquals("SchedulePassword", stored.purpose); assertEquals("Intent", stored.state)
            assertThrows(NotebookSchedules.Unavailable::class.java) { backend.schedules().retireUnusedPassword(stored.id, NativeBridge.Verification { false }, true).await() }
            assertTrue(backend.schedules().retireUnusedPassword(stored.id, proof, true).await().acknowledged)
            assertEquals("Deleted", backend.schedules().storedCredentials().await().credentials.single().state)
        } finally { backend.closeFullyAsync().get(20, TimeUnit.SECONDS) }
    }
    @Test fun noKeyOfferOrMissingStorageConsentCannotPublishStoreCredentialsOrRegisterJob() {
        val root = temporary.newFolder(); val destination = temporary.newFolder(); val jobs = Jobs(); val keys = Keys(); val backend = backend(root, jobs, keys)
        try {
            create(backend); val stage = prepare(backend, NotebookSchedules.Target.Directory(destination))
            assertThrows(NotebookSchedules.Unavailable::class.java) { backend.schedules().enable(stage, proof, true, true, false, true).await() }
            offered(stage)
            assertThrows(NotebookSchedules.Unavailable::class.java) { backend.schedules().enable(stage, proof, true, false, false, true).await() }
            assertTrue(keys.keys.isEmpty()); assertEquals(0, jobs.registrations); assertTrue(destination.listFiles()!!.isEmpty())
            stage.close()
        } finally { backend.closeFullyAsync().get(20, TimeUnit.SECONDS) }
    }
    @Test fun authoringInvalidatesPreparedSecretsAndCannotPublishAnObsoleteSnapshot() {
        val root = temporary.newFolder(); val destination = temporary.newFolder(); val jobs = Jobs(); val keys = Keys(); val backend = backend(root, jobs, keys)
        try {
            create(backend); val stage = prepare(backend, NotebookSchedules.Target.Directory(destination)); offered(stage)
            create(backend)
            assertThrows(IllegalStateException::class.java) { stage.copyRecoveryKey() }
            assertThrows(NotebookSchedules.Unavailable::class.java) { backend.schedules().enable(stage, proof, true, true, false, true).await() }
            assertTrue(keys.keys.isEmpty()); assertEquals(0, jobs.registrations); assertTrue(destination.listFiles()!!.isEmpty())
        } finally { backend.closeFullyAsync().get(20, TimeUnit.SECONDS) }
    }
    @Test fun uncertainRegistrationKeepsKnownInitialPublicationAndNeedsExplicitReconciliation() {
        val root = temporary.newFolder(); val destination = temporary.newFolder(); val jobs = Jobs().apply { result = BackupJobScheduler.Registration.Uncertain }
        val backend = backend(root, jobs, Keys())
        try {
            create(backend); val stage = prepare(backend, NotebookSchedules.Target.Directory(destination)); offered(stage)
            val receipt = backend.schedules().enable(stage, proof, true, true, false, true).await()
            assertEquals(NotebookSchedules.Publication.Published, receipt.initialPublication); assertTrue(receipt.needsAttention)
            assertEquals(NotebookSchedules.State.Arming, receipt.configuration!!.state)
            assertEquals("Ignored", backend.schedules().runDue(jobs.token!!).await().result)
            val enabled = backend.schedules().reconcileRegistration(receipt.configuration.revision, proof, true).await()
            assertEquals(NotebookSchedules.State.Enabled, enabled.state); assertEquals(1, jobs.registrations)
        } finally { backend.closeFullyAsync().get(20, TimeUnit.SECONDS) }
    }
    @Test fun providerGrantAndReadbackAreExplicitAndNeverBecomeACloudDurabilityClaim() {
        val root = temporary.newFolder(); val documents = Documents(temporary.newFolder()); val permissions = Permissions(); val jobs = Jobs(); val backend = backend(root, jobs, Keys(), permissions, documents)
        val tree = Uri.parse("content://synthetic.schedules/tree/root")
        val flags = Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
        try {
            create(backend); val stage = prepare(backend, NotebookSchedules.Target.Provider(tree, flags)); offered(stage)
            val receipt = backend.schedules().enable(stage, proof, true, true, true, true).await()
            assertEquals(NotebookSchedules.Publication.ProviderVerified, receipt.initialPublication)
            assertEquals("Persisted", receipt.persistentAccessOutcome); assertEquals(1, permissions.takes); assertEquals(1, documents.files.size)
            assertEquals("ProviderVerified", backend.schedules().runDue(jobs.token!!).await().result)
            assertEquals(1, permissions.takes); assertEquals(2, documents.files.size)
        } finally { backend.closeFullyAsync().get(20, TimeUnit.SECONDS) }
    }
    @Test fun replacementDirectoryIsNotSilentlyAcceptedByAnEnabledSchedule() {
        val root = temporary.newFolder(); val destination = temporary.newFolder(); val jobs = Jobs(); val keys = Keys(); val backend = backend(root, jobs, keys)
        try {
            create(backend); val stage = prepare(backend, NotebookSchedules.Target.Directory(destination)); offered(stage)
            backend.schedules().enable(stage, proof, true, true, false, true).await()
            val renamed = File(destination.parentFile, destination.name + "-original"); assertTrue(destination.renameTo(renamed)); assertTrue(destination.mkdir())
            val due = backend.schedules().runDue(jobs.token!!).await()
            assertEquals("Failed", due.result); assertNull(due.backup); assertEquals(0, keys.decrypts)
            assertTrue(destination.listFiles()!!.isEmpty()); assertEquals(1, renamed.listFiles()!!.size)
        } finally { backend.closeFullyAsync().get(20, TimeUnit.SECONDS) }
    }
    @Test fun quotaFailureCreatesNoExternalBackupCredentialOrSchedule() {
        val root = temporary.newFolder(); val destination = temporary.newFolder(); val jobs = Jobs(); val keys = Keys(); val backend = backend(root, jobs, keys)
        try {
            create(backend); val secret = password
            try { assertThrows(NotebookSchedules.Unavailable::class.java) {
                backend.schedules().prepare(NotebookSchedules.Options(NotebookSchedules.Target.Directory(destination), 1000, 1), secret, proof).await()
            } } finally { secret.fill('\u0000') }
            assertEquals(0, jobs.registrations); assertTrue(keys.keys.isEmpty()); assertTrue(destination.listFiles()!!.isEmpty())
            assertTrue(JSONObject(backend.request("""{"version":1,"command":{"op":"Status"}}""").get(10, TimeUnit.SECONDS)).getBoolean("ok"))
        } finally { backend.closeFullyAsync().get(20, TimeUnit.SECONDS) }
    }
    private fun command(backend: NotebookBackend, value: String): JSONObject =
        JSONObject(backend.request("""{"version":1,"command":$value}""").get(10, TimeUnit.SECONDS))
    private fun protect(backend: NotebookBackend, note: UUID) {
        backend.authenticate("""[{"Entity":"$note"},"App"]""", proof).get(10, TimeUnit.SECONDS)
        assertTrue(command(backend, """{"op":"SetLock","entity":"$note","revision":0,"locked":true}""").getBoolean("ok"))
        assertTrue(command(backend, """{"op":"SetAppLock","enabled":true}""").getBoolean("ok"))
    }
    @Test fun protectedBackupNeedsSeparateConsentAndLeavesAppAndNoteReadsLocked() {
        val root = temporary.newFolder(); val destination = temporary.newFolder(); val jobs = Jobs(); val keys = Keys(); val backend = backend(root, jobs, keys)
        try {
            val note = create(backend); protect(backend, note)
            val stage = prepare(backend, NotebookSchedules.Target.Directory(destination)); offered(stage)
            assertTrue(stage.requiresProtectedContentConsent)
            assertThrows(NotebookSchedules.Unavailable::class.java) { backend.schedules().enable(stage, proof, true, true, false, true).await() }
            assertTrue(destination.listFiles()!!.isEmpty()); assertTrue(keys.keys.isEmpty()); assertEquals(0, jobs.registrations)
            val initial = backend.schedules().enable(stage, proof, true, true, false, true, true).await()
            assertEquals(NotebookSchedules.Publication.Published, initial.initialPublication)
            assertTrue(command(backend, """{"op":"Relock"}""").getBoolean("ok"))
            assertFalse(command(backend, """{"op":"Read","id":"$note"}""").getBoolean("ok"))
            assertEquals("Published", backend.schedules().runDue(jobs.token!!).await().result)
            assertEquals(2, destination.listFiles()!!.size)
            assertFalse(command(backend, """{"op":"Read","id":"$note"}""").getBoolean("ok"))
            assertThrows(NotebookBackups.Failure::class.java) { backend.backups().history().await() }
        } finally { backend.closeFullyAsync().get(20, TimeUnit.SECONDS) }
    }
    @Test fun freshConsentedPreparationNeedsNoOrdinaryNoteUnlockAndDoesNotRunOnWriter() {
        val root = temporary.newFolder(); val destination = temporary.newFolder(); val jobs = Jobs(); val backend = backend(root, jobs, Keys())
        try {
            val note = create(backend); protect(backend, note)
            command(backend, """{"op":"Relock"}""")
            backend.authenticate("""["App"]""", proof).get(10, TimeUnit.SECONDS)
            assertFalse(command(backend, """{"op":"Read","id":"$note"}""").getBoolean("ok"))
            assertThrows(NotebookSchedules.Unavailable::class.java) { prepare(backend, NotebookSchedules.Target.Directory(destination)) }
            val calls = java.util.concurrent.atomic.AtomicInteger()
            val fresh = NativeBridge.Verification { challenge ->
                UUID.fromString(challenge); calls.incrementAndGet()
                assertTrue(command(backend, """{"op":"Status"}""").getBoolean("ok")); true
            }
            val secret = password
            val stage = try { backend.schedules().prepare(NotebookSchedules.Options(NotebookSchedules.Target.Directory(destination), 1000, 10000000), secret, fresh, allowProtectedContent = true).await() }
                finally { secret.fill('\u0000') }
            assertEquals(1, calls.get()); assertTrue(stage.requiresProtectedContentConsent); offered(stage)
            assertFalse(command(backend, """{"op":"Read","id":"$note"}""").getBoolean("ok"))
            assertTrue(destination.listFiles()!!.isEmpty()); assertNull(jobs.token)
            val receipt = backend.schedules().enable(stage, proof, true, true, false, true, true).await()
            assertEquals(NotebookSchedules.Publication.Published, receipt.initialPublication)
            assertFalse(command(backend, """{"op":"Read","id":"$note"}""").getBoolean("ok"))
        } finally { backend.closeFullyAsync().get(20, TimeUnit.SECONDS) }
    }
    @Test fun declinedFreshProtectedPreparationCreatesNoPublicationCredentialOrJob() {
        val root = temporary.newFolder(); val destination = temporary.newFolder(); val jobs = Jobs(); val keys = Keys(); val backend = backend(root, jobs, keys)
        try {
            val note = create(backend); protect(backend, note)
            val secret = password
            try { assertThrows(NotebookSchedules.Unavailable::class.java) {
                backend.schedules().prepare(NotebookSchedules.Options(NotebookSchedules.Target.Directory(destination), 1000, 10000000), secret, NativeBridge.Verification { false }, allowProtectedContent = true).await()
            } } finally { secret.fill('\u0000') }
            assertTrue(destination.listFiles()!!.isEmpty()); assertTrue(keys.keys.isEmpty()); assertEquals(0, jobs.registrations)
        } finally { backend.closeFullyAsync().get(20, TimeUnit.SECONDS) }
    }
    @Test fun protectedScopeExpansionRejectsOldDelegationWithoutPublishingOrUnlocking() {
        val root = temporary.newFolder(); val destination = temporary.newFolder(); val jobs = Jobs(); val backend = backend(root, jobs, Keys())
        try {
            val note = create(backend)
            val stage = prepare(backend, NotebookSchedules.Target.Directory(destination)); offered(stage)
            backend.schedules().enable(stage, proof, true, true, false, true).await()
            protect(backend, note)
            command(backend, """{"op":"Relock"}""")
            assertEquals("Failed", backend.schedules().runDue(jobs.token!!).await().result)
            assertEquals(1, destination.listFiles()!!.size)
            assertFalse(command(backend, """{"op":"Read","id":"$note"}""").getBoolean("ok"))
        } finally { backend.closeFullyAsync().get(20, TimeUnit.SECONDS) }
    }
    @Test fun sameRevisionPlainPolicyTamperingCannotBroadenEncryptedDelegation() {
        val root = temporary.newFolder(); val destination = temporary.newFolder(); val jobs = Jobs(); val backend = backend(root, jobs, Keys())
        try {
            val note = create(backend); protect(backend, note)
            val stage = prepare(backend, NotebookSchedules.Target.Directory(destination)); offered(stage)
            backend.schedules().enable(stage, proof, true, true, false, true, true).await()
            command(backend, """{"op":"Relock"}""")
            val policy = File(root, "notebook/backup-policy/policy.db")
            android.database.sqlite.SQLiteDatabase.openDatabase(policy.path, null, android.database.sqlite.SQLiteDatabase.NO_LOCALIZED_COLLATORS).use {
                it.execSQL("UPDATE schedule_limits SET maximum_bytes=9000000")
            }
            assertEquals("Failed", backend.schedules().runDue(jobs.token!!).await().result)
            assertEquals(1, destination.listFiles()!!.size)
            assertFalse(command(backend, """{"op":"Read","id":"$note"}""").getBoolean("ok"))
        } finally { backend.closeFullyAsync().get(20, TimeUnit.SECONDS) }
    }
    @Test fun grantPossibleEffectReturnsUncertainReceiptWithoutBackupOrGuessedPermissionRollback() {
        val root = temporary.newFolder(); val documents = Documents(temporary.newFolder()); val permissions = Permissions().apply { afterTake = { documents.available = false } }
        val jobs = Jobs(); val keys = Keys(); val backend = backend(root, jobs, keys, permissions, documents)
        val tree = Uri.parse("content://synthetic.schedules/tree/root")
        val flags = Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
        try {
            create(backend); val stage = prepare(backend, NotebookSchedules.Target.Provider(tree, flags)); offered(stage)
            val receipt = backend.schedules().enable(stage, proof, true, true, true, true).await()
            assertEquals("Uncertain", receipt.persistentAccessOutcome); assertNotNull(receipt.permissionDecision); assertTrue(receipt.needsAttention)
            assertNull(receipt.initialPublication); assertNull(receipt.configuration); assertTrue(keys.keys.isEmpty())
            assertTrue(permissions.held.complete); assertEquals(0, jobs.registrations); assertTrue(documents.files.isEmpty())
        } finally { backend.closeFullyAsync().get(20, TimeUnit.SECONDS) }
    }
}
