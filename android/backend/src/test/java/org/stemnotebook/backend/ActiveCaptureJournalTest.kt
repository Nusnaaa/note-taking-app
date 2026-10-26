package org.stemnotebook.backend

import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.File
import java.io.IOException
import java.util.UUID
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

class ActiveCaptureJournalTest {
    @get:Rule val temporary = TemporaryFolder()
    private val library = UUID.randomUUID(); private val entity = UUID.randomUUID()
    private fun point(time: Long) = ObservedPoint(-0.0, 1.2345678901234567, time, 0.0, null, -0.125, 0)
    private fun stroke() = CapturedStroke(UUID.randomUUID().toString(), UUID.randomUUID().toString(), 0, 7, false, listOf(point(10)), StrokeEnd.InProgress)
    @Test fun versionOneGoldenRecordRecoversWithoutInventingStyleAndUpgradesOnlyRecoveryMetadata() {
        val root = temporary.newFolder(); val legacyLibrary = UUID(0, 1); val ticket = UUID(0, 3)
        val bytes = java.util.Base64.getDecoder().decode("U1RFTUFDMDEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAACAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADAAAAAAAAAAAAAAAAAAAABAAAAAAAAAAAAAAABwAAAAAAAAALZml4ZWQtc3BhY2UAAAABgAAAAAAAAAA/9AAAAAAAAAAAAAAAAAAKAQAAAAAAAAAApuxK8dPXXeVxPEqmelAqAxd64nz0rv5XsNbTGk90u2Q=")
        val file = File(root, "0000000000000000000-$ticket.prefix").apply { writeBytes(bytes) }
        val journal = ActiveCaptureJournal(root, legacyLibrary); val before = journal.read(ticket); assertNull(before.stroke.style)
        assertEquals((-0.0).toRawBits(), before.stroke.points.single().x.toRawBits()); assertEquals(0.0, before.stroke.points.single().pressure!!, 0.0)
        journal.recoverActive(); assertNull(journal.read(ticket).stroke.style); assertEquals(before.stroke.points, journal.read(ticket).stroke.points)
        assertEquals("STEMAC05", String(file.readBytes().copyOfRange(0, 8), Charsets.US_ASCII))
        assertEquals(0, journal.drain { error("Recovered prefix requires review") })
    }
    @Test fun versionFiveRetainsChosenResponseAndExactAbsentZeroObservationsAcrossRecovery() {
        val root = temporary.newFolder(); val response = PressureResponse(0.0, 2.0, 0.25, 1.25, 2.0)
        val chosen = StrokeStyle(32, 64, 128, 128, 8.0, response)
        val original = stroke().copy(style = chosen, points = listOf(point(10).copy(pressure = null), point(20).copy(pressure = 0.0)))
        val ticket = UUID.fromString(original.id)
        ActiveCaptureJournal(root, library).checkpoint(entity, "fixed-space", 0, original, layer = CaptureLayer.Base)
        val reopened = ActiveCaptureJournal(root, library); reopened.recoverActive(); val actual = reopened.read(ticket)
        assertEquals(chosen, actual.stroke.style); assertEquals(original.points, actual.stroke.points)
        assertEquals(8.0, actual.stroke.style!!.widthFor(null), 0.0); assertEquals(2.0, actual.stroke.style!!.widthFor(0.0), 0.0)
        reopened.approve(ticket, reopened.checksum(ticket))
        assertEquals(1, reopened.drain { entry -> assertEquals(original.points, entry.stroke.points); assertEquals(chosen, entry.stroke.style); true })
    }
    @Test fun versionTwoGoldenRecordPreservesStyleWithoutInventingADestination() {
        val root = temporary.newFolder(); val legacyLibrary = UUID(0, 1); val ticket = UUID(0, 3)
        File(root, "0000000000000000000-$ticket.prefix").writeBytes(java.util.Base64.getDecoder().decode("U1RFTUFDMDIAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAACAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADAAAAAAAAAAAAAAAAAAAABAAAAAAAAAAAAAAABwAAAAABEYD/QD/jMzMzMzMzAAAAC2ZpeGVkLXNwYWNlAAAAAYAAAAAAAAAAP/QAAAAAAAAAAAAAAAAACgEAAAAAAAAAAPjd+K3N+C37gfeypj9fNmJ6X5oi3zh8uKlj6iJtFwIj"))
        val journal = ActiveCaptureJournal(root, legacyLibrary)
        assertEquals(StrokeStyle(17, 128, 255, 64, 0.6), journal.read(ticket).stroke.style); assertNull(journal.read(ticket).layer)
        journal.recoverActive(); assertNull(journal.read(ticket).layer)
        assertEquals(StrokeStyle(17, 128, 255, 64, 0.6), journal.read(ticket).stroke.style)
    }
    @Test fun layerDestinationIsDurableAndCannotBeReboundByLaterCheckpointsOrRecovery() {
        val root = temporary.newFolder(); val source = stroke(); val ticket = UUID.fromString(source.id); val layer = CaptureLayer.Custom(UUID.randomUUID())
        val journal = ActiveCaptureJournal(root, library); journal.checkpoint(entity, "fixed-space", 0, source, layer = layer)
        assertThrows(IllegalArgumentException::class.java) { journal.checkpoint(entity, "fixed-space", 1, source, layer = CaptureLayer.Base) }
        val reopened = ActiveCaptureJournal(root, library); reopened.recoverActive(); assertEquals(layer, reopened.read(ticket).layer)
        val entry = reopened.read(ticket); assertEquals(source.points, entry.stroke.points)
        reopened.approve(ticket, reopened.checksum(ticket)); assertEquals(1, reopened.drain { assertEquals(layer, it.layer); true })
        val base = stroke(); reopened.checkpoint(entity, "fixed-space", 0, base, layer = CaptureLayer.Base)
        assertEquals(CaptureLayer.Base, reopened.read(UUID.fromString(base.id)).layer)
    }
    @Test fun reviewedRetargetPreservesSealedObservationsAndRejectsActiveStaleOrImplicitRebinding() {
        val root = temporary.newFolder(); val journal = ActiveCaptureJournal(root, library); val active = stroke(); val ticket = UUID.fromString(active.id)
        val old = CaptureLayer.Custom(UUID.randomUUID()); val selected = CaptureLayer.Custom(UUID.randomUUID())
        journal.checkpoint(entity, "fixed-space", 0, active, layer = old)
        assertThrows(IllegalArgumentException::class.java) { journal.retarget(ticket, journal.checksum(ticket), selected) }
        val sealed = active.copy(end = StrokeEnd.Interrupted(StrokeInterruption.Lifecycle), style = null)
        journal.checkpoint(entity, "fixed-space", 1, sealed, layer = old)
        val checksum = journal.checksum(ticket)
        assertThrows(IllegalArgumentException::class.java) { journal.retarget(ticket, "stale", selected) }
        val changed = journal.retarget(ticket, checksum, selected)
        assertEquals(sealed, changed.stroke); assertEquals(selected, changed.layer); assertEquals(2, changed.revision)
        assertThrows(IllegalArgumentException::class.java) { journal.retarget(ticket, checksum, CaptureLayer.Base) }
        assertThrows(IllegalArgumentException::class.java) { journal.checkpoint(entity, "fixed-space", 3, sealed, layer = old) }
        val reopened = ActiveCaptureJournal(root, library); assertEquals(changed, reopened.read(ticket))
        assertEquals(1, reopened.drain { assertEquals(selected, it.layer); assertEquals(sealed, it.stroke); true })
    }
    @Test fun retargetPublicationFailureRetainsAnHonestRecoverableDestinationAndExactStroke() {
        for (phase in listOf("after-file-sync", "after-rename")) {
            val root = temporary.newFolder(); val armed = AtomicBoolean(false)
            val journal = ActiveCaptureJournal(root, library, fault = { if (it == phase && armed.getAndSet(false)) throw IOException("synthetic retarget failure") })
            val source = stroke().copy(end = StrokeEnd.Interrupted(StrokeInterruption.Recovery))
            val ticket = UUID.fromString(source.id); val old = CaptureLayer.Custom(UUID.randomUUID())
            journal.checkpoint(entity, "fixed-space", 0, source, layer = old); armed.set(true)
            assertThrows(IOException::class.java) { journal.retarget(ticket, journal.checksum(ticket), CaptureLayer.Base) }
            val reopened = ActiveCaptureJournal(root, library); val entry = reopened.read(ticket)
            assertEquals(source, entry.stroke); assertTrue(entry.review)
            assertEquals(if (phase == "after-rename") CaptureLayer.Base else old, entry.layer)
            assertEquals(0, reopened.drain { error("Recovered evidence still requires approval") })
        }
    }
    @Test fun styleSurvivesBinaryRecoveryAndCannotBeReboundByAChangedCheckpointHeader() {
        val root = temporary.newFolder(); val chosen = StrokeStyle(17, 128, 255, 64, 0.12345678901234567); val source = stroke().copy(style = chosen); val ticket = UUID.fromString(source.id)
        val journal = ActiveCaptureJournal(root, library); journal.checkpoint(entity, "fixed-space", 0, source)
        assertThrows(IllegalArgumentException::class.java) { journal.checkpoint(entity, "fixed-space", 1, source.copy(style = chosen.copy(width = 2.0))) }
        val recovered = ActiveCaptureJournal(root, library); recovered.recoverActive(); assertEquals(chosen, recovered.read(ticket).stroke.style)
        recovered.approve(ticket, recovered.checksum(ticket)); assertEquals(1, recovered.drain { assertEquals(chosen, it.stroke.style); assertEquals(source.points, it.stroke.points); true })
    }
    @Test fun explicitFingerProvenanceSurvivesRecoveryAndCannotBeReboundToPen() {
        val root = temporary.newFolder(); val source = stroke().copy(explicitFinger = true); val ticket = UUID.fromString(source.id)
        val journal = ActiveCaptureJournal(root, library); journal.checkpoint(entity, "fixed-space", 0, source, layer = CaptureLayer.Base)
        assertThrows(IllegalArgumentException::class.java) { journal.checkpoint(entity, "fixed-space", 1, source.copy(explicitFinger = false), layer = CaptureLayer.Base) }
        val recovered = ActiveCaptureJournal(root, library); recovered.recoverActive()
        val entry = recovered.read(ticket); assertTrue(entry.stroke.explicitFinger); assertEquals(source.points, entry.stroke.points); assertEquals(CaptureLayer.Base, entry.layer)
        recovered.approve(ticket, recovered.checksum(ticket))
        assertEquals(1, recovered.drain { assertTrue(it.stroke.explicitFinger); assertTrue(CaptureRequest.encode(ticket, entity, "fixed-space", listOf(it.stroke)).contains("ExplicitFinger")); true })
        assertThrows(IllegalArgumentException::class.java) { source.copy(eraser = true) }
    }
    @Test fun versionThreeRetainsPenAndLayerWithoutInventingFingerInput() {
        val root = temporary.newFolder(); val source = stroke(); val ticket = UUID.fromString(source.id)
        ActiveCaptureJournal(root, library).checkpoint(entity, "fixed-space", 0, source, layer = CaptureLayer.Base)
        // v3 and v4 pen/eraser payloads share the same one-byte tool representation.
        val file = root.listFiles()!!.single(); val bytes = file.readBytes(); bytes[7] = '3'.code.toByte()
        val payload = bytes.copyOfRange(0, bytes.size - 32)
        file.writeBytes(payload + java.security.MessageDigest.getInstance("SHA-256").digest(payload))
        val reopened = ActiveCaptureJournal(root, library); val before = reopened.read(ticket)
        assertFalse(before.stroke.explicitFinger); assertEquals(source, before.stroke); assertEquals(CaptureLayer.Base, before.layer)
        reopened.recoverActive(); assertFalse(reopened.read(ticket).stroke.explicitFinger)
    }
    @Test fun checkpointsPreserveExactPrefixAndRejectStaleChangedOrReboundUpdates() {
        val root = temporary.newFolder(); val journal = ActiveCaptureJournal(root, library); val source = stroke(); val ticket = UUID.fromString(source.id)
        assertThrows(java.nio.charset.CharacterCodingException::class.java) { journal.checkpoint(entity, 0xD800.toChar().toString(), 0, source) }
        assertEquals(0, journal.pendingCount())
        journal.checkpoint(entity, "fixed-space", 0, source)
        val extended = source.copy(points = source.points + point(20)); journal.checkpoint(entity, "fixed-space", 1, extended)
        assertEquals(extended, journal.read(ticket).stroke)
        assertEquals((-0.0).toRawBits(), journal.read(ticket).stroke.points[0].x.toRawBits())
        assertThrows(IllegalArgumentException::class.java) { journal.checkpoint(entity, "fixed-space", 0, source) }
        assertThrows(IllegalArgumentException::class.java) { journal.checkpoint(entity, "fixed-space", 2, extended.copy(points = listOf(point(11), point(20)))) }
        assertThrows(IllegalArgumentException::class.java) { journal.checkpoint(UUID.randomUUID(), "fixed-space", 2, extended) }
        assertThrows(IllegalArgumentException::class.java) { journal.checkpoint(entity, "changed-space", 2, extended) }
        val complete = extended.copy(end = StrokeEnd.Completed); journal.checkpoint(entity, "fixed-space", 2, complete)
        journal.checkpoint(entity, "fixed-space", 2, complete)
        assertThrows(IllegalArgumentException::class.java) { journal.checkpoint(entity, "fixed-space", 3, complete.copy(points = complete.points + point(30))) }
        assertEquals(1, journal.drain { assertEquals(complete, it.stroke); true }); assertEquals(0, journal.pendingCount())
    }
    @Test fun recoveryIsExplicitInterruptedEvidenceAndCannotPublishBeforeReview() {
        val root = temporary.newFolder(); val source = stroke(); val ticket = UUID.fromString(source.id)
        ActiveCaptureJournal(root, library).checkpoint(entity, "fixed-space", 0, source)
        val recovered = ActiveCaptureJournal(root, library); recovered.recoverActive()
        val entry = recovered.read(ticket); assertEquals(StrokeEnd.Interrupted(StrokeInterruption.Recovery), entry.stroke.end); assertEquals(source.points, entry.stroke.points)
        assertEquals(0, recovered.drain { error("Unreviewed active prefix must not publish") }); assertEquals(1, recovered.reviewCount())
        assertThrows(IllegalArgumentException::class.java) { recovered.approve(ticket, "wrong checksum") }
        recovered.approve(ticket, recovered.checksum(ticket))
        assertEquals(1, recovered.drain { assertEquals(entry.stroke, it.stroke); true }); assertEquals(0, recovered.pendingCount())
    }
    @Test fun nativeFailureAndAmbiguousAcknowledgementKeepExactSealedRecordForRetry() {
        val root = temporary.newFolder(); val armed = AtomicBoolean(false)
        val journal = ActiveCaptureJournal(root, library, fault = { phase -> if (phase == "after-native-reply" && armed.getAndSet(false)) throw IOException("synthetic acknowledgement loss") })
        val source = stroke().copy(end = StrokeEnd.Completed); journal.checkpoint(entity, "fixed-space", 0, source)
        assertEquals(0, journal.drain { false }); assertEquals(1, journal.pendingCount())
        var deliveries = 0; val committed = mutableSetOf<String>()
        armed.set(true)
        assertThrows(IOException::class.java) { journal.drain { deliveries++; committed.add(it.stroke.id); true } }
        assertEquals(1, journal.pendingCount())
        val reopened = ActiveCaptureJournal(root, library)
        assertEquals(1, reopened.drain { assertEquals(source, it.stroke); deliveries++; committed.add(it.stroke.id); true })
        assertEquals(2, deliveries); assertEquals(1, committed.size)
    }
    @Test fun sealedBlockedRecordsAreReviewableWithoutChangingTheirObservedEndingOrRequiringApprovalMutation() {
        val journal = ActiveCaptureJournal(temporary.newFolder(), library)
        val source = stroke().copy(end = StrokeEnd.Completed); val ticket = UUID.fromString(source.id)
        journal.checkpoint(entity, "fixed-space", 0, source)
        assertEquals(listOf(ticket), journal.reviewTickets()); assertEquals(0, journal.drain { false })
        val checksum = journal.checksum(ticket); journal.approve(ticket, checksum)
        assertEquals(checksum, journal.checksum(ticket)); assertEquals(source, journal.read(ticket).stroke)
        assertThrows(IllegalArgumentException::class.java) { journal.discardReview(ticket, "stale") }
        journal.discardReview(ticket, checksum); assertEquals(0, journal.pendingCount())
        val active = stroke(); val activeTicket = UUID.fromString(active.id)
        journal.checkpoint(entity, "fixed-space", 0, active)
        assertTrue(journal.reviewTickets().isEmpty())
        assertThrows(IllegalArgumentException::class.java) { journal.discardReview(activeTicket, journal.checksum(activeTicket)) }
        assertThrows(IllegalArgumentException::class.java) { journal.approve(activeTicket, journal.checksum(activeTicket)) }
    }
    @Test fun cancellationNeverSubmitsTentativeContactAndQuotaDoesNotEvictOlderInk() {
        val root = temporary.newFolder(); val source = stroke(); val journal = ActiveCaptureJournal(root, library, maximumRecords = 1)
        journal.checkpoint(entity, "fixed-space", 0, source)
        assertThrows(IllegalArgumentException::class.java) { journal.checkpoint(entity, "fixed-space", 0, stroke()) }
        assertEquals(source, journal.read(UUID.fromString(source.id)).stroke)
        journal.checkpoint(entity, "fixed-space", 1, source, cancelled = true)
        assertEquals(0, journal.drain { error("Cancelled tentative contact must not publish") }); assertEquals(0, journal.pendingCount())
    }
    @Test fun unreviewedOrActiveSessionDoesNotBlockIndependentCompletedInput() {
        val root = temporary.newFolder(); val journal = ActiveCaptureJournal(root, library)
        val active = stroke(); journal.checkpoint(entity, "fixed-space", 0, active)
        val independent = stroke().copy(end = StrokeEnd.Completed); journal.checkpoint(entity, "fixed-space", 0, independent)
        assertEquals(1, journal.drain { assertEquals(independent, it.stroke); true })
        assertEquals(1, journal.pendingCount()); assertEquals(active, journal.read(UUID.fromString(active.id)).stroke)
    }
    @Test fun malformedAuthoritativeDataAndForeignLibraryBindingFailWithoutDeletingEvidence() {
        val root = temporary.newFolder(); val source = stroke(); ActiveCaptureJournal(root, library).checkpoint(entity, "fixed-space", 0, source)
        val file = root.listFiles()!!.single(); val original = file.readBytes()
        assertThrows(IllegalArgumentException::class.java) { ActiveCaptureJournal(root, UUID.randomUUID()) }; assertArrayEquals(original, file.readBytes())
        file.writeBytes(original.copyOf().also { it[20] = (it[20].toInt() xor 1).toByte() })
        assertThrows(IllegalArgumentException::class.java) { ActiveCaptureJournal(root, library) }; assertTrue(file.exists())
    }
    @Test fun tornUnacknowledgedTemporaryIsPreservedSeparatelyFromValidCheckpoint() {
        val root = temporary.newFolder(); val source = stroke(); ActiveCaptureJournal(root, library).checkpoint(entity, "fixed-space", 0, source)
        val torn = File(root, "active-torn.pending").apply { writeBytes(byteArrayOf(1, 2, 3)) }
        val journal = ActiveCaptureJournal(root, library)
        assertFalse(torn.exists()); assertEquals(1, journal.incompleteCount()); assertEquals(source, journal.read(UUID.fromString(source.id)).stroke)
        assertArrayEquals(byteArrayOf(1, 2, 3), File(root, "active-torn.incomplete").readBytes())
    }
    @Test(timeout = 30_000) fun actualProcessExitAtFileBoundariesRecoversLastValidObservedPrefix() {
        val classes = listOf(ActiveCaptureJournal::class.java, CaptureJournalCrashProbe::class.java, kotlin.Unit::class.java).map { File(checkNotNull(checkNotNull(it.protectionDomain).codeSource).location.toURI()).absolutePath }.distinct().joinToString(File.pathSeparator)
        for (phase in listOf("after-file-sync", "after-rename", "after-directory-sync")) {
            val root = temporary.newFolder(); val source = stroke(); val ticket = UUID.fromString(source.id)
            ActiveCaptureJournal(root, library).checkpoint(entity, "fixed-space", 0, source)
            val child = ProcessBuilder(File(System.getProperty("java.home"), "bin/java").absolutePath, "-cp", classes, CaptureJournalCrashProbe::class.java.name, root.absolutePath, library.toString(), ticket.toString(), phase).redirectErrorStream(true).start()
            try { assertTrue(child.waitFor(10, TimeUnit.SECONDS)); val output = child.inputStream.bufferedReader().readText(); assertEquals(output, 81, child.exitValue()) } finally { child.destroyForcibly() }
            val recovered = ActiveCaptureJournal(root, library); recovered.recoverActive(); val entry = recovered.read(ticket)
            assertEquals(2, entry.stroke.points.size); assertEquals(StrokeEnd.Interrupted(StrokeInterruption.Recovery), entry.stroke.end)
            assertEquals(source.points[0], entry.stroke.points[0]); assertEquals(11L, entry.stroke.points[1].timeUs)
            assertEquals(0, recovered.drain { error("Recovered prefix requires review") })
        }
    }
}

class CaptureJournalCrashProbe {
    companion object {
        @JvmStatic fun main(args: Array<String>) {
            val journal = ActiveCaptureJournal(File(args[0]), UUID.fromString(args[1]), fault = { phase -> if (phase == args[3]) Runtime.getRuntime().halt(81) })
            val entry = journal.read(UUID.fromString(args[2])); val next = entry.stroke.copy(points = entry.stroke.points + entry.stroke.points.last().copy(timeUs = entry.stroke.points.last().timeUs + 1))
            journal.checkpoint(entry.entity, entry.space, entry.revision + 1, next)
            error("Crash boundary was not reached")
        }
    }
}
