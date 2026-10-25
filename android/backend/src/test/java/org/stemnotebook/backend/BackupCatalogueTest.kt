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

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class BackupCatalogueTest {
    @get:Rule val temporary = TemporaryFolder()
    private val library = UUID.randomUUID().toString()
    private fun folder() = File(temporary.newFolder(), "catalogue")
    private fun snapshot(generation: String = "0", quiet: Boolean = true) = BackupSnapshot(library, generation, quiet)
    private fun append(catalogue: BackupCatalogue, generation: String = "0") = catalogue.append(snapshot(generation), BackupDestinationKind.LocalFile,
        File(temporary.root, "explicit-encrypted-backup.stemenc").absolutePath, 123, "a".repeat(64), BackupAcknowledgement.LocalDurable, true)
    private fun unavailable(action: () -> Unit) {
        try { action(); fail("Invalid publication history accepted") }
        catch (expected: IOException) { assertEquals("Backup publication history unavailable", expected.message) }
    }
    @Test fun acknowledgedSnapshotsSurviveReopenWithoutConflatingPendingOrNewEdits() {
        val folder = folder(); val catalogue = BackupCatalogue(folder)
        assertNull(catalogue.status(snapshot()).lastPublication)
        val first = append(catalogue)
        val reopened = BackupCatalogue(folder)
        assertEquals(first, reopened.status(snapshot()).lastPublication)
        assertTrue(reopened.status(snapshot()).currentAuthoringWasPublished)
        assertFalse(reopened.status(snapshot(quiet = false)).currentAuthoringWasPublished)
        assertFalse(reopened.status(snapshot("1")).currentAuthoringWasPublished)
        val newer = append(reopened, "1")
        append(reopened, "0") // Publishing an old artifact must not hide a newer match.
        val status = reopened.status(snapshot("1"))
        assertEquals("0", status.lastPublication!!.generation)
        assertEquals(newer, status.currentCommittedSnapshotPublication)
        assertTrue(status.currentAuthoringWasPublished)
        assertNull(reopened.status(BackupSnapshot(UUID.randomUUID().toString(), "1", true)).lastPublication)
    }
    @Test fun providerReadbackIsRecordedAsSuchWithoutClaimingRemoteDurabilityOrContactingIt() {
        val catalogue = BackupCatalogue(folder())
        val record = catalogue.append(snapshot(), BackupDestinationKind.ProviderDocument, "content://unavailable.example/document/explicit-backup", 42,
            "b".repeat(64), BackupAcknowledgement.ProviderReadBack, false)
        assertEquals(BackupAcknowledgement.ProviderReadBack, catalogue.status(snapshot()).lastPublication!!.acknowledgement)
        assertFalse(record.descriptorSyncAcknowledged)
        assertTrue(catalogue.status(snapshot()).currentAuthoringWasPublished)
    }
    @Test fun inclusionAndOmissionFactsRemainAvailableAfterRestart() {
        val folder = folder(); val catalogue = BackupCatalogue(folder)
        val omissions = listOf("External source bytes excluded; relationship retained.", "Optional model resources excluded; reinstall locally.")
        val record = catalogue.append(snapshot(), BackupDestinationKind.LocalFile, "/explicit/backup.stemenc", 1,
            "a".repeat(64), BackupAcknowledgement.LocalDurable, true, true, 2, omissions)
        val reopened = BackupCatalogue(folder).status(snapshot()).lastPublication!!
        assertEquals(record, reopened); assertTrue(reopened.includesOriginalInk)
        assertEquals(2L, reopened.sourceAssets); assertEquals(omissions, reopened.omissions)
    }
    @Test fun failedMetadataCommitCannotAdvanceTheRecordedSnapshot() {
        val folder = folder(); val original = BackupCatalogue(folder); val first = append(original)
        val failing = BackupCatalogue(folder) { if (it == "catalogue-before-commit") throw IOException("injected") }
        unavailable { append(failing, "1") }
        val state = BackupCatalogue(folder).status(snapshot("1"))
        assertEquals(first, state.lastPublication); assertFalse(state.currentAuthoringWasPublished)
    }
    @Test fun lostPostCommitAcknowledgementKeepsTheCommittedReceiptForRestartDiscovery() {
        val folder = folder()
        val failing = BackupCatalogue(folder) { if (it == "catalogue-after-commit") throw IOException("injected") }
        unavailable { append(failing) }
        assertTrue(BackupCatalogue(folder).status(snapshot()).currentAuthoringWasPublished)
    }
    @Test fun pageCursorsStayBoundedAndDoNotRepeatNewlyAddedReceipts() {
        val catalogue = BackupCatalogue(folder()); repeat(8) { append(catalogue, it.toString()) }
        val first = catalogue.page(library, null, 3)
        assertEquals(3, first.size)
        append(catalogue, "9")
        val second = catalogue.page(library, first.last().sequence, 3)
        val third = catalogue.page(library, second.last().sequence, 3)
        assertEquals(8, (first + second + third).map { it.id }.toSet().size)
        assertTrue(catalogue.page(library, third.last().sequence, 3).isEmpty())
        unavailable { catalogue.page(library, null, 101) }
        unavailable { catalogue.page(library, 0, 1) }
    }
    @Test fun existingForeignOrFutureDatabasesArePreservedWithoutInitialisation() {
        for (future in listOf(false, true)) {
            val folder = folder(); Files.createDirectory(folder.toPath(), PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rwx------")))
            val file = File(folder, "catalogue.db")
            SQLiteDatabase.openOrCreateDatabase(file, null).use { db ->
                db.execSQL("CREATE TABLE original(value TEXT)"); db.execSQL("INSERT INTO original VALUES('preserved')"); db.version = if (future) 2 else 1
            }
            Files.setPosixFilePermissions(file.toPath(), PosixFilePermissions.fromString("rw-------"))
            val before = file.readBytes()
            unavailable { BackupCatalogue(folder).status(snapshot()) }
            assertArrayEquals(before, file.readBytes())
        }
    }
    @Test fun linksAndDirectoryReplacementAreRejectedBeforeOpeningSqlite() {
        for (hard in listOf(false, true)) {
            val folder = folder(); Files.createDirectory(folder.toPath(), PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rwx------")))
            val original = File(temporary.newFolder(), "original").apply { writeText("preserve unknown bytes") }
            Files.setPosixFilePermissions(original.toPath(), PosixFilePermissions.fromString("rw-------"))
            val linked = File(folder, "catalogue.db").toPath()
            if (hard) Files.createLink(linked, original.toPath()) else Files.createSymbolicLink(linked, original.toPath())
            unavailable { BackupCatalogue(folder).status(snapshot()) }
            assertEquals("preserve unknown bytes", original.readText())
        }
        val folder = folder(); val catalogue = BackupCatalogue(folder); append(catalogue)
        Files.move(folder.toPath(), File(folder.parentFile, "preserved").toPath())
        Files.createDirectory(folder.toPath(), PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rwx------")))
        unavailable { catalogue.status(snapshot()) }
        assertFalse(File(folder, "catalogue.db").exists())
    }
    @Test fun unknownNamespacesAndMissingIndexesAreNotModifiedOrRepaired() {
        val folder = folder(); Files.createDirectory(folder.toPath(), PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rwx------")))
        val unknown = File(folder, "unclassified").apply { writeText("preserve") }
        unavailable { BackupCatalogue(folder).status(snapshot()) }
        assertFalse(File(folder, "catalogue.db").exists()); assertEquals("preserve", unknown.readText())
        val valid = folder(); val catalogue = BackupCatalogue(valid); append(catalogue)
        SQLiteDatabase.openDatabase(File(valid, "catalogue.db").path, null, SQLiteDatabase.OPEN_READWRITE or SQLiteDatabase.NO_LOCALIZED_COLLATORS).use { it.execSQL("DROP INDEX publications_snapshot") }
        unavailable { catalogue.status(snapshot()) }
    }
    @Test fun trustedAppAncestorAliasesWorkButCannotRedirectAnAnchoredCatalogue() {
        val parent = temporary.newFolder(); File(parent, "notebook").mkdir()
        val alias = File(temporary.newFolder(), "app-alias"); Files.createSymbolicLink(alias.toPath(), parent.toPath())
        val catalogue = BackupCatalogue(File(alias, "notebook/catalogue")); append(catalogue)
        assertTrue(catalogue.status(snapshot()).currentAuthoringWasPublished)
        val replacement = temporary.newFolder(); File(replacement, "notebook").mkdir()
        Files.delete(alias.toPath()); Files.createSymbolicLink(alias.toPath(), replacement.toPath())
        unavailable { catalogue.status(snapshot()) }
        assertFalse(File(replacement, "notebook/catalogue").exists())
    }
    @Test fun corruptedReceiptFieldsFailInsteadOfBeingSilentlyRepaired() {
        val folder = folder(); val catalogue = BackupCatalogue(folder); append(catalogue)
        SQLiteDatabase.openDatabase(File(folder, "catalogue.db").path, null, SQLiteDatabase.OPEN_READWRITE or SQLiteDatabase.NO_LOCALIZED_COLLATORS).use { db ->
            db.execSQL("UPDATE publications SET descriptor_synced=2")
        }
        unavailable { catalogue.status(snapshot()) }
    }
    @Test fun exhaustedSequenceCannotWrapOrAdvanceTheRecordedSnapshot() {
        val folder = folder(); val catalogue = BackupCatalogue(folder); val first = append(catalogue)
        SQLiteDatabase.openDatabase(File(folder, "catalogue.db").path, null, SQLiteDatabase.OPEN_READWRITE or SQLiteDatabase.NO_LOCALIZED_COLLATORS).use { db ->
            db.execSQL("UPDATE sqlite_sequence SET seq=? WHERE name='publications'", arrayOf(Long.MAX_VALUE))
        }
        unavailable { append(catalogue, "1") }
        assertEquals(first, catalogue.status(snapshot("1")).lastPublication)
        assertFalse(catalogue.status(snapshot("1")).currentAuthoringWasPublished)
    }
    @Test fun invalidSnapshotDestinationAndAcknowledgementCannotCreateReceipts() {
        val catalogue = BackupCatalogue(folder())
        unavailable { catalogue.append(snapshot("-1"), BackupDestinationKind.LocalFile, "/explicit", 1, "a".repeat(64), BackupAcknowledgement.LocalDurable, true) }
        unavailable { catalogue.append(snapshot(), BackupDestinationKind.LocalFile, "relative", 1, "a".repeat(64), BackupAcknowledgement.LocalDurable, true) }
        unavailable { catalogue.append(snapshot(), BackupDestinationKind.ProviderDocument, "file://wrong", 1, "a".repeat(64), BackupAcknowledgement.ProviderReadBack, false) }
        unavailable { catalogue.append(snapshot(), BackupDestinationKind.LocalFile, "/explicit", 1, "a".repeat(64), BackupAcknowledgement.ProviderReadBack, true) }
        unavailable { catalogue.append(snapshot(), BackupDestinationKind.LocalFile, "/explicit", 1, "a".repeat(64), BackupAcknowledgement.LocalDurable, false) }
        assertNull(catalogue.status(snapshot()).lastPublication)
    }
}
