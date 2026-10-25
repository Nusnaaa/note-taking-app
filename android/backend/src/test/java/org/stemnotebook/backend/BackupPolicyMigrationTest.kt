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
import java.util.UUID

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class BackupPolicyMigrationTest {
    @get:Rule val temporary = TemporaryFolder()
    private val library = UUID.randomUUID()
    private fun fixture(): Pair<File, BackupSchedulePolicy.Configuration> {
        val directory = File(temporary.newFolder(), "policy"); val store = BackupPolicyStore(directory)
        val key = BackupSecretContext(library, UUID.randomUUID(), BackupSecretContext.Purpose.SchedulePassword)
        store.beginCredential(key, null, true, true)
        store.commitCredential(key, "STEMKEY1".toByteArray() + byteArrayOf(1) + ByteArray(30))
        val policy = BackupSchedulePolicy(directory)
        val arming = policy.arm(library, null, BackupSchedulePolicy.Destination.LocalDirectory, "/explicit/backups", key.id, null, 123456L, true, false, true, 12345, "synthetic-directory-identity")
        return directory to policy.registrationAcknowledged(library, arming.revision)
    }
    private fun legacy(directory: File) {
        SQLiteDatabase.openDatabase(File(directory, "policy.db").path, null, SQLiteDatabase.NO_LOCALIZED_COLLATORS).use { db ->
            db.execSQL("DROP TABLE schedule_limits"); db.version = 1
        }
    }
    @Test fun exactLegacyMigrationPreservesCredentialsAndRequiresFreshLimitAndSchedulingReview() {
        val (directory, enabled) = fixture(); val key = BackupSecretContext(library, enabled.credential, BackupSecretContext.Purpose.SchedulePassword)
        val before = BackupPolicyStore(directory).credential(key)!!.envelope!!
        legacy(directory)
        BackupPolicyDatabase(directory).access(false) { assertEquals(2, it.version) }
        val policy = BackupSchedulePolicy(directory); val disabled = policy.configuration(library)!!
        assertEquals(BackupSchedulePolicy.State.Disabled, disabled.state)
        assertNotEquals(enabled.revision, disabled.revision); assertNull(disabled.maximumBytes)
        assertArrayEquals(before, BackupPolicyStore(directory).credential(key)!!.envelope)
        assertNull(policy.start(library, enabled.id, enabled.revision))
        val new = policy.arm(library, disabled.revision, disabled.kind, disabled.destination, disabled.credential, null, disabled.intervalMillis, true, false, true, 98765, "reviewed-new-identity")
        assertEquals(98765L, new.maximumBytes); assertEquals(BackupSchedulePolicy.State.Arming, new.state)
    }
    @Test fun migrationKeepsUnfinishedRunForExclusiveRestartReconciliation() {
        val (directory, enabled) = fixture(); val old = BackupSchedulePolicy(directory)
        val run = old.start(library, enabled.id, enabled.revision)!!
        legacy(directory)
        BackupPolicyDatabase(directory).access(false) { assertEquals(2, it.version) }
        val reopened = BackupSchedulePolicy(directory); val disabled = reopened.configuration(library)!!
        assertEquals(run.id, disabled.activeRun); assertEquals(BackupSchedulePolicy.State.Disabled, disabled.state)
        assertEquals(BackupSchedulePolicy.Result.Interrupted, reopened.reconcileInterrupted(library)!!.result)
        assertNull(reopened.configuration(library)!!.activeRun)
    }
    @Test fun futureSchemaIsPreservedWithoutDowngradeOrMigration() {
        val (directory, _) = fixture(); val file = File(directory, "policy.db")
        SQLiteDatabase.openDatabase(file.path, null, SQLiteDatabase.NO_LOCALIZED_COLLATORS).use { it.version = 3 }
        val bytes = file.readBytes()
        assertThrows(BackupPolicyStore.Unavailable::class.java) { BackupSchedulePolicy(directory).configuration(library) }
        assertArrayEquals(bytes, file.readBytes())
    }
    @Test fun invalidLegacyIdentityRollsBackTheWholeMigration() {
        val (directory, _) = fixture(); legacy(directory)
        val file = File(directory, "policy.db")
        SQLiteDatabase.openDatabase(file.path, null, SQLiteDatabase.NO_LOCALIZED_COLLATORS).use { it.execSQL("UPDATE configurations SET id='invalid'") }
        assertThrows(BackupPolicyStore.Unavailable::class.java) { BackupSchedulePolicy(directory).configuration(library) }
        SQLiteDatabase.openDatabase(file.path, null, SQLiteDatabase.OPEN_READONLY).use { db ->
            assertEquals(1, db.version)
            db.rawQuery("SELECT count(*) FROM sqlite_master WHERE name='schedule_limits'", null).use { assertTrue(it.moveToFirst()); assertEquals(0, it.getInt(0)) }
            db.rawQuery("SELECT state FROM configurations", null).use { assertTrue(it.moveToFirst()); assertEquals("Enabled", it.getString(0)) }
        }
    }
}
