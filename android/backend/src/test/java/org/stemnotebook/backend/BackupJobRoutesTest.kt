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
import java.nio.file.Files
import java.nio.file.attribute.PosixFilePermissions
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class BackupJobRoutesTest {
    @get:Rule val temporary = TemporaryFolder()
    private fun token() = BackupJobScheduler.Token(UUID.randomUUID(), UUID.randomUUID())
    private fun unavailable(action: () -> Unit) { assertThrows(BackupJobRoutes.Unavailable::class.java, action) }
    @Test fun absentReadCreatesNoMapAndOnlyLegacyPrimaryHasAFallback() {
        val primary = temporary.newFolder(); val routes = BackupJobRoutes(primary)
        assertNull(routes.job(BackupJobScheduler.JOB_ID)); assertNull(routes.root(primary))
        assertEquals(primary.canonicalPath, routes.resolve(BackupJobScheduler.JOB_ID, token()).root.path)
        unavailable { routes.resolve(BackupJobScheduler.JOB_ID + 1, token()) }
        assertFalse(File(primary, "backup-job-routes").exists())
    }
    @Test fun distinctRootsPersistDistinctSlotsAndExactRevisionSurvivesRestartAndAliases() {
        val primary = temporary.newFolder(); val other = temporary.newFolder(); val routes = BackupJobRoutes(primary)
        val library = UUID.randomUUID(); val firstToken = token()
        val first = routes.reserve(BackupJobRoutes.Root.inspect(primary), UUID.randomUUID(), token()) { true }
        val second = routes.reserve(BackupJobRoutes.Root.inspect(other), library, firstToken) { true }
        assertEquals(BackupJobScheduler.JOB_ID, first.job); assertNotEquals(first.job, second.job)
        val restarted = BackupJobRoutes(primary)
        assertEquals(second, restarted.root(File(other, "."))); assertEquals(first, restarted.job(first.job))
        val next = firstToken.copy(revision = UUID.randomUUID())
        val renewed = restarted.reserve(BackupJobRoutes.Root.inspect(other), library, next) { true }
        assertEquals(second.job, renewed.job)
        unavailable { restarted.resolve(second.job, firstToken) }
        assertEquals(BackupJobRoutes.Resolved(renewed.root, library), restarted.resolve(renewed.job, next))
    }
    @Test fun replacedRootOrAnotherLibraryCannotAdoptAnExistingRoute() {
        val primary = temporary.newFolder(); val other = temporary.newFolder(); val routes = BackupJobRoutes(primary)
        val entry = routes.reserve(BackupJobRoutes.Root.inspect(other), UUID.randomUUID(), token()) { true }
        unavailable { routes.reserve(entry.root, UUID.randomUUID(), entry.token) { true } }
        val retained = File(other.parentFile, other.name + "-original"); check(other.renameTo(retained)); check(other.mkdir())
        unavailable { routes.resolve(entry.job, entry.token) }
        unavailable { routes.reserve(BackupJobRoutes.Root.inspect(other), entry.library, entry.token) { true } }
        assertEquals(entry, routes.job(entry.job)); assertFalse(File(other, "notebook").exists())
    }
    @Test fun anotherPathWithTheSameLibraryIdentityCannotEvictOrDuplicateItsRoute() {
        val primary = temporary.newFolder(); val other = temporary.newFolder(); val routes = BackupJobRoutes(primary)
        val library = UUID.randomUUID(); val original = routes.reserve(BackupJobRoutes.Root.inspect(primary), library, token()) { true }
        unavailable { routes.reserve(BackupJobRoutes.Root.inspect(other), library, token()) { true } }
        assertEquals(original, routes.job(original.job)); assertNull(routes.root(other))
        assertEquals(original.root, routes.resolve(original.job, original.token).root)
    }
    @Test fun foreignSlotsAreSkippedAndExhaustionNeverEvictsOrOverwrites() {
        val primary = temporary.newFolder(); val routes = BackupJobRoutes(primary); val usable = BackupJobScheduler.JOB_ID + 2
        val first = routes.reserve(BackupJobRoutes.Root.inspect(temporary.newFolder()), UUID.randomUUID(), token()) { it == usable }
        assertEquals(usable, first.job)
        unavailable { routes.reserve(BackupJobRoutes.Root.inspect(temporary.newFolder()), UUID.randomUUID(), token()) { it == usable } }
        assertEquals(first, routes.job(usable)); assertNull(routes.job(BackupJobScheduler.JOB_ID + 1))
    }
    @Test fun independentControllersSerializeFirstInitializationAndSlotReservation() {
        val primary = temporary.newFolder(); val a = temporary.newFolder(); val b = temporary.newFolder()
        val start = CountDownLatch(1); val pool = Executors.newFixedThreadPool(2)
        try {
            val work = listOf(a, b).map { root -> pool.submit<BackupJobRoutes.Route> {
                check(start.await(10, TimeUnit.SECONDS)); BackupJobRoutes(primary).reserve(BackupJobRoutes.Root.inspect(root), UUID.randomUUID(), token()) { true }
            } }
            start.countDown(); val found = work.map { it.get(10, TimeUnit.SECONDS) }
            assertNotEquals(found[0].job, found[1].job)
            found.forEach { assertEquals(it, BackupJobRoutes(primary).job(it.job)) }
        } finally { start.countDown(); pool.shutdownNow() }
    }
    @Test fun unknownAndFutureSchemasRemainByteForBytePreserved() {
        for (future in listOf(false, true)) {
            val primary = temporary.newFolder(); val routes = BackupJobRoutes(primary)
            val directory = File(primary, "backup-job-routes"); val file = File(directory, "routes.db")
            if (future) {
                routes.reserve(BackupJobRoutes.Root.inspect(primary), UUID.randomUUID(), token()) { true }
                SQLiteDatabase.openDatabase(file.path, null, SQLiteDatabase.NO_LOCALIZED_COLLATORS).use { it.version = 2 }
            } else {
                Files.createDirectory(directory.toPath(), PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rwx------")))
                SQLiteDatabase.openDatabase(file.path, null, SQLiteDatabase.CREATE_IF_NECESSARY or SQLiteDatabase.NO_LOCALIZED_COLLATORS).use {
                    it.execSQL("CREATE TABLE original(value TEXT)"); it.version = 1
                }
                Files.setPosixFilePermissions(file.toPath(), PosixFilePermissions.fromString("rw-------"))
            }
            val bytes = file.readBytes()
            unavailable { routes.job(BackupJobScheduler.JOB_ID) }
            unavailable { routes.reserve(BackupJobRoutes.Root.inspect(primary), UUID.randomUUID(), token()) { true } }
            assertArrayEquals(bytes, file.readBytes())
        }
    }
    @Test fun overflowingIntegerMetadataCannotAliasAnOwnedJobOrUid() {
        for (column in listOf("job", "uid")) {
            val primary = temporary.newFolder(); val routes = BackupJobRoutes(primary)
            routes.reserve(BackupJobRoutes.Root.inspect(primary), UUID.randomUUID(), token()) { true }
            val file = File(primary, "backup-job-routes/routes.db")
            SQLiteDatabase.openDatabase(file.path, null, SQLiteDatabase.NO_LOCALIZED_COLLATORS).use {
                it.execSQL("UPDATE routes SET $column=$column+4294967296")
            }
            val bytes = file.readBytes()
            unavailable { routes.root(primary) }
            assertArrayEquals(bytes, file.readBytes())
        }
    }
    @Test fun linkedFilesAndReplacedRegistryNamespaceAreUnavailableWithoutRepair() {
        val primary = temporary.newFolder(); val routes = BackupJobRoutes(primary)
        routes.reserve(BackupJobRoutes.Root.inspect(primary), UUID.randomUUID(), token()) { true }
        val directory = File(primary, "backup-job-routes"); val file = File(directory, "routes.db")
        val linked = File(primary, "retained-hardlink"); Files.createLink(linked.toPath(), file.toPath())
        unavailable { routes.job(BackupJobScheduler.JOB_ID) }; check(linked.delete())
        val retained = File(primary, "retained-registry"); check(directory.renameTo(retained))
        Files.createDirectory(directory.toPath(), PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rwx------")))
        unavailable { routes.job(BackupJobScheduler.JOB_ID) }
        assertFalse(File(directory, "routes.db").exists()); assertTrue(File(retained, "routes.db").exists())
    }
}
