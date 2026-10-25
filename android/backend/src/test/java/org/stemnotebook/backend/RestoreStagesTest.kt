package org.stemnotebook.backend

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
import java.util.concurrent.atomic.AtomicBoolean

/** Actual host filesystem semantics; no Android device acceptance claim. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class RestoreStagesTest {
    @get:Rule val temporary = TemporaryFolder()
    private val library = UUID.randomUUID().toString()
    private fun manager(fault: (String) -> Unit = {}): Triple<RestoreStages, File, File> {
        val parent = temporary.newFolder()
        val pending = File(parent, "pending"); val retired = File(parent, "retired")
        return Triple(RestoreStages(pending, retired, fault), pending, retired)
    }
    private fun payload(stage: RestoreStages.Stage, bytes: ByteArray = byteArrayOf(1, 2, 3)): File {
        val file = File(stage.directory, ".stem-stream-restore-${UUID.randomUUID()}.db")
        Files.write(file.toPath(), bytes)
        Files.setPosixFilePermissions(file.toPath(), PosixFilePermissions.fromString("rw-------"))
        return file
    }
    private fun snapshot(stages: RestoreStages, retired: Boolean = false, maximumFiles: Int = 64): RestoreStages.Snapshot {
        val page = stages.page(retired, null, 32) {}
        assertEquals(1, page.entries.size)
        return stages.capture(page.entries.single(), library, maximumFiles) {}
    }
    @Test fun knownPrivateStageRequiresReviewAndDiscardsOnlyItsOwnFiles() {
        val (stages, pending, retired) = manager()
        val other = temporary.newFile().apply { writeText("healthy unrelated data") }
        val stage = stages.create(library); payload(stage)
        val selected = snapshot(stages)
        assertTrue(selected.recognised); assertTrue(selected.complete)
        var entered = false
        val result = stages.act(selected, library, 64, RestoreStages.Disposition.DiscardRecognised, {}, { entered = true })
        assertTrue(entered); assertEquals(RestoreStages.Outcome.Discarded, result.outcome)
        assertEquals(selected.observedBytes, result.removedBytes)
        assertTrue(result.sourceRetired); assertTrue(pending.list()!!.isEmpty()); assertTrue(retired.list()!!.isEmpty())
        assertEquals("healthy unrelated data", other.readText())
    }
    @Test fun markerAndFilenameAloneCannotAuthorizeDeletionOfUnclassifiedData() {
        val (stages, pending, retired) = manager()
        val unknown = File(pending, "unclassified").apply { writeBytes(byteArrayOf(3, 5, 7)) }
        val selected = snapshot(stages)
        assertFalse(selected.recognised); assertEquals(3L, selected.observedBytes)
        try { stages.act(selected, library, 64, RestoreStages.Disposition.DiscardRecognised, {}, {}); fail("Unknown deleted") } catch (_: IllegalArgumentException) { }
        val result = stages.act(selected, library, 64, RestoreStages.Disposition.Quarantine, {}, {})
        assertEquals(RestoreStages.Outcome.Quarantined, result.outcome); assertEquals(0L, result.removedBytes)
        assertFalse(unknown.exists()); assertArrayEquals(byteArrayOf(3, 5, 7), File(retired, "${result.retirement}/entry").readBytes())
        assertFalse(snapshot(stages, true).recognised)
    }
    @Test fun changedPayloadWithRestoredSizeAndModificationTimeCannotUseTheOldReview() {
        val (stages, _, _) = manager(); val stage = stages.create(library); val file = payload(stage)
        val selected = snapshot(stages)
        val modified = Files.getLastModifiedTime(file.toPath())
        file.writeBytes(byteArrayOf(7, 8, 9)); Files.setLastModifiedTime(file.toPath(), modified)
        try { stages.act(selected, library, 64, RestoreStages.Disposition.DiscardRecognised, {}, {}); fail("Changed data deleted") } catch (_: RestoreStages.Changed) { }
        assertArrayEquals(byteArrayOf(7, 8, 9), file.readBytes())
    }
    @Test fun changedMarkerCannotReuseAnOldReview() {
        val (stages, _, _) = manager(); val stage = stages.create(library); payload(stage)
        val selected = snapshot(stages)
        val marker = File(stage.directory, "stage.json")
        marker.writeText(marker.readText().replace(library, UUID.randomUUID().toString()))
        try { stages.act(selected, library, 64, RestoreStages.Disposition.DiscardRecognised, {}, {}); fail("Changed marker accepted") } catch (_: RestoreStages.Changed) { }
        assertFalse(snapshot(stages).recognised)
    }
    @Test fun symlinksAndHardlinksStayUnclassifiedAndNeverFollowTheirTargets() {
        val (stages, _, retired) = manager(); val stage = stages.create(library)
        val outside = temporary.newFile().apply { writeText("preserve target") }
        val link = File(stage.directory, ".stem-stream-restore-${UUID.randomUUID()}.db")
        Files.createSymbolicLink(link.toPath(), outside.toPath())
        val selected = snapshot(stages); assertFalse(selected.recognised); assertNull(selected.observedBytes)
        val result = stages.act(selected, library, 64, RestoreStages.Disposition.Quarantine, {}, {})
        assertTrue(Files.isSymbolicLink(File(retired, "${result.retirement}/entry/${link.name}").toPath()))
        assertEquals("preserve target", outside.readText())
        val next = stages.create(library)
        Files.createLink(File(next.directory, ".stem-stream-restore-${UUID.randomUUID()}.db").toPath(), outside.toPath())
        assertFalse(snapshot(stages).recognised)
    }
    @Test fun renamedQuarantineCanBeReviewedAndDiscardedAfterRestart() {
        val once = AtomicBoolean(true)
        val (stages, pending, retired) = manager { if (it == "retirement-moved" && once.getAndSet(false)) throw IOException("synthetic interruption") }
        val stage = stages.create(library); payload(stage)
        val result = stages.act(snapshot(stages), library, 64, RestoreStages.Disposition.DiscardRecognised, {}, {})
        assertEquals(RestoreStages.Outcome.Uncertain, result.outcome); assertTrue(result.sourceRetired); assertEquals(0L, result.removedBytes)
        assertTrue(pending.list()!!.isEmpty())
        val reopened = RestoreStages(pending, retired)
        val selected = snapshot(reopened, true); assertTrue(selected.recognised)
        val completed = reopened.act(selected, library, 64, RestoreStages.Disposition.DiscardRecognised, {}, {})
        assertEquals(RestoreStages.Outcome.Discarded, completed.outcome); assertTrue(retired.list()!!.isEmpty())
    }
    @Test fun interruptedMetadataCleanupIsRecognisedWithoutInventingRemainingPayload() {
        val once = AtomicBoolean(true)
        val (stages, pending, retired) = manager { if (it == "retirement-before-record-cleanup" && once.getAndSet(false)) throw IOException("synthetic interruption") }
        val stage = stages.create(library); payload(stage)
        val result = stages.act(snapshot(stages), library, 64, RestoreStages.Disposition.DiscardRecognised, {}, {})
        assertEquals(RestoreStages.Outcome.Uncertain, result.outcome); assertTrue(result.removedBytes > 0)
        assertTrue(pending.list()!!.isEmpty())
        val selected = snapshot(stages, true); assertTrue(selected.recognised); assertTrue(selected.metadataOnly)
        assertEquals(listOf("retirement.json"), selected.children.map { it.name })
        assertEquals(RestoreStages.Outcome.Discarded, stages.act(selected, library, 64, RestoreStages.Disposition.DiscardRecognised, {}, {}).outcome)
        assertTrue(retired.list()!!.isEmpty())
    }
    @Test fun preCommitFaultPreservesTheSourceAndRemovesOnlyTheEmptyReservation() {
        val (stages, pending, retired) = manager { if (it == "retirement-before-move") throw IOException("synthetic failure") }
        val stage = stages.create(library); payload(stage)
        try { stages.act(snapshot(stages), library, 64, RestoreStages.Disposition.DiscardRecognised, {}, {}); fail("Fault ignored") } catch (_: IOException) { }
        assertTrue(stage.directory.isDirectory); assertEquals(1, pending.list()!!.size); assertTrue(retired.list()!!.isEmpty())
    }
    @Test fun payloadChangedAfterTransferIsPreservedWithAnUncertainReceipt() {
        lateinit var movedPayload: File
        lateinit var retiredRoot: File
        val (stages, _, retired) = manager { if (it == "retirement-moved") {
            movedPayload = retiredRoot.walkTopDown().first { file -> file.name.endsWith(".db") }
            movedPayload.writeBytes(byteArrayOf(4, 5, 6))
        } }
        retiredRoot = retired
        val stage = stages.create(library); payload(stage)
        val result = stages.act(snapshot(stages), library, 64, RestoreStages.Disposition.DiscardRecognised, {}, {})
        assertEquals(RestoreStages.Outcome.Uncertain, result.outcome); assertEquals(0L, result.removedBytes)
        assertArrayEquals(byteArrayOf(4, 5, 6), movedPayload.readBytes())
    }
    @Test fun boundedPaginationAndIncompleteInspectionsNeverGrantDeletion() {
        val (stages, pending, _) = manager()
        for (name in listOf("a", "b", "c", "d", "e")) File(pending, name).writeText(name)
        val first = stages.page(false, null, 2) {}; assertEquals(listOf("a", "b"), first.entries.map { it.name }); assertEquals("b", first.next)
        assertEquals(listOf("c", "d"), stages.page(false, first.next, 2) {}.entries.map { it.name })
        val stage = stages.create(library); repeat(5) { payload(stage) }
        val selected = stages.capture(RestoreStages.Entry(stage.directory.name, false), library, 2) {}
        assertFalse(selected.complete); assertFalse(selected.recognised); assertNull(selected.observedBytes); assertEquals(2, selected.children.size)
        assertEquals(RestoreStages.Outcome.Quarantined, stages.act(selected, library, 2, RestoreStages.Disposition.Quarantine, {}, {}).outcome)
    }
    @Test fun interruptedEmptyReservationsCanBeReviewedWithoutDeletingUnknownContent() {
        val (stages, pending, retired) = manager()
        val empty = File(pending, "empty-before-marker"); assertTrue(empty.mkdir())
        Files.setPosixFilePermissions(empty.toPath(), PosixFilePermissions.fromString("rwx------"))
        val selected = snapshot(stages); assertTrue(selected.recognised); assertTrue(selected.emptyDirectory)
        assertEquals(0L, selected.observedBytes)
        assertEquals(RestoreStages.Outcome.Discarded, stages.act(selected, library, 64, RestoreStages.Disposition.DiscardRecognised, {}, {}).outcome)
        val abandoned = File(retired, UUID.randomUUID().toString()); assertTrue(abandoned.mkdir())
        Files.setPosixFilePermissions(abandoned.toPath(), PosixFilePermissions.fromString("rwx------"))
        val record = snapshot(stages, true); assertTrue(record.emptyDirectory); assertTrue(record.recognised)
        assertEquals(RestoreStages.Outcome.Discarded, stages.act(record, library, 64, RestoreStages.Disposition.DiscardRecognised, {}, {}).outcome)
        assertTrue(pending.list()!!.isEmpty()); assertTrue(retired.list()!!.isEmpty())
    }
    @Test fun replacementOfTheOwnedRootCannotRedirectInspectionIntoHealthyFiles() {
        val (stages, pending, _) = manager()
        val healthy = temporary.newFolder()
        val file = File(healthy, "library.db").apply { writeBytes(byteArrayOf(4, 5, 6)) }
        val original = File(pending.parentFile, "original-pending")
        Files.move(pending.toPath(), original.toPath())
        Files.createSymbolicLink(pending.toPath(), healthy.toPath())
        try { stages.page(false, null, 32) {}; fail("Replacement root followed") } catch (_: RestoreStages.Changed) { }
        assertArrayEquals(byteArrayOf(4, 5, 6), file.readBytes()); assertTrue(original.isDirectory)
    }
    @Test fun unknownMarkerExtensionsOrVersionsCannotGrantPermanentDiscard() {
        val (stages, _, _) = manager(); val stage = stages.create(library); payload(stage)
        val marker = File(stage.directory, "stage.json")
        val original = marker.readText()
        marker.writeText(org.json.JSONObject(original).put("retain_new_content", true).toString())
        assertFalse(snapshot(stages).recognised)
        marker.writeText(org.json.JSONObject(original).put("format", 1.5).toString())
        assertFalse(snapshot(stages).recognised)
    }
    @Test fun onlyAMatchingPrivateLibraryMarkerCanClassifyScratch() {
        val (stages, _, _) = manager(); val stage = stages.create(UUID.randomUUID().toString()); payload(stage)
        assertFalse(snapshot(stages).recognised)
        try { stages.capture(RestoreStages.Entry("../healthy", false), library, 64) {}; fail("Traversal accepted") } catch (_: IllegalArgumentException) { }
    }
}
