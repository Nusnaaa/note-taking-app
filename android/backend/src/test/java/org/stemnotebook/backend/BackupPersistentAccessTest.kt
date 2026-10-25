package org.stemnotebook.backend

import android.content.Intent
import android.net.Uri
import android.os.CancellationSignal
import android.os.ParcelFileDescriptor
import android.provider.DocumentsContract
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.io.IOException
import java.util.UUID

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class BackupPersistentAccessTest {
    private val flags = Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
    private val tree get() = DocumentsContract.buildTreeDocumentUri("synthetic.backup", "selected")
    private class Documents : BackupDocuments() {
        var label = "Selected backup directory"
        var inspected = 0
        var inspectAction: () -> Unit = {}
        override fun inspect(tree: Uri, signal: CancellationSignal): Selection { signal.throwIfCanceled(); inspected++; inspectAction(); return Selection("selected", label) }
        override fun create(tree: Uri, name: String): Uri = error("Consent must not create a backup")
        override fun isChild(tree: Uri, document: Uri): Boolean = error("No backup document")
        override fun open(document: Uri, write: Boolean, signal: CancellationSignal): ParcelFileDescriptor = error("Consent must not open notebook/ciphertext bytes")
    }
    private class Permissions : BackupPersistentPermissions() {
        var held = Held(false, false)
        var taken = 0
        var takeAction: () -> Unit = {}
        override fun current(tree: Uri) = held
        override fun take(tree: Uri) { taken++; held = Held(true, true); takeAction() }
    }
    private fun unavailable(action: () -> Unit) {
        try { action(); fail("Invalid persistent grant accepted") }
        catch (expected: IOException) { assertEquals("Backup destination consent unavailable", expected.message) }
    }
    @Test fun explicitReviewedConsentIsRecordedBeforePersistentGrantRequest() {
        val docs = Documents(); val permissions = Permissions(); val manager = BackupPersistentAccess(UUID.randomUUID(), docs, permissions) {}
        val signal = CancellationSignal(); val review = manager.review(tree, flags, signal) {}
        assertEquals(0, permissions.taken); assertEquals("Selected backup directory", review.label)
        val order = mutableListOf<String>(); permissions.takeAction = { order.add("take") }
        val receipt = manager.accept(review, true, true, { UUID.fromString(it); order.add("proof"); true }, signal, {},
            { assertEquals(0, permissions.taken); order.add("decision") }, { order.add("outcome") })
        assertEquals(listOf("proof", "decision", "take", "outcome"), order)
        assertEquals(BackupPersistentAccess.Outcome.Persisted, receipt.outcome)
        assertEquals(true, receipt.observedRead); assertEquals(true, receipt.observedWrite)
        assertEquals(false, receipt.decision.previousRead)
        unavailable { manager.accept(review, true, true, { true }, signal, {}, {}, {}) }
    }
    @Test fun missingPickerRightsAndRejectedConsentCannotTakePermission() {
        for (offered in listOf(0, Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)) {
            val permissions = Permissions(); val manager = BackupPersistentAccess(UUID.randomUUID(), Documents(), permissions) {}
            unavailable { manager.review(tree, offered, CancellationSignal()) {} }; assertEquals(0, permissions.taken)
        }
        for ((confirmed, allowed, authorised) in listOf(Triple(false,true,true), Triple(true,false,true), Triple(true,true,false))) {
            val permissions = Permissions(); val manager = BackupPersistentAccess(UUID.randomUUID(), Documents(), permissions) {}
            val signal = CancellationSignal(); val review = manager.review(tree, flags, signal) {}
            unavailable { manager.accept(review, confirmed, allowed, { authorised }, signal, {}, { fail("Rejected consent recorded") }, {}) }
            assertEquals(0, permissions.taken)
        }
    }
    @Test fun changedAuthorityDirectoryOrPermissionInvalidatesReviewBeforeAnyGrant() {
        for (change in 0..2) {
            val docs = Documents(); val permissions = Permissions(); var authorised = true
            val manager = BackupPersistentAccess(UUID.randomUUID(), docs, permissions) { check(authorised) }
            val signal = CancellationSignal(); val review = manager.review(tree, flags, signal) {}
            when(change) { 0 -> authorised = false; 1 -> docs.label = "Changed"; else -> permissions.held = BackupPersistentPermissions.Held(true,false) }
            unavailable { manager.accept(review, true, true, { true }, signal, {}, {}, {}) }
            assertEquals(0, permissions.taken)
        }
    }
    @Test fun preexistingCompleteAccessIsNotRetakenOrTreatedAsExclusiveOwnership() {
        val permissions = Permissions(); permissions.held = BackupPersistentPermissions.Held(true,true)
        val manager = BackupPersistentAccess(UUID.randomUUID(), Documents(), permissions) {}
        val signal = CancellationSignal(); val review = manager.review(tree, 0, signal) {}
        val receipt = manager.accept(review, true, true, { true }, signal, {}, {}, {})
        assertEquals(BackupPersistentAccess.Outcome.AlreadyHeld, receipt.outcome)
        assertTrue(receipt.decision.previousRead && receipt.decision.previousWrite); assertEquals(0, permissions.taken)
    }
    @Test fun failedConsentRecordingDoesNotTakeAPersistedGrant() {
        val permissions = Permissions(); val manager = BackupPersistentAccess(UUID.randomUUID(), Documents(), permissions) {}
        val signal = CancellationSignal(); val review = manager.review(tree, flags, signal) {}
        unavailable { manager.accept(review, true, true, { true }, signal, {}, { throw IOException("injected metadata failure") }, {}) }
        assertEquals(0, permissions.taken); assertFalse(permissions.held.complete)
    }
    @Test fun failureAfterGrantEntryPreservesPossibleEffectsWithoutRevokingExistingAccess() {
        val permissions = Permissions(); permissions.held = BackupPersistentPermissions.Held(true,false)
        permissions.takeAction = { throw IOException("post-effect failure") }
        val manager = BackupPersistentAccess(UUID.randomUUID(), Documents(), permissions) {}
        val signal = CancellationSignal(); val review = manager.review(tree, flags, signal) {}
        val receipt = manager.accept(review, true, true, { true }, signal, {}, {}, {})
        assertEquals(BackupPersistentAccess.Outcome.Uncertain, receipt.outcome)
        assertEquals(true, receipt.observedRead); assertEquals(true, receipt.observedWrite)
        assertTrue(receipt.decision.previousRead); assertFalse(receipt.decision.previousWrite)
        assertTrue(permissions.held.complete)
    }
    @Test fun cancellationAndLostOutcomeRecordingAfterGrantCannotHideTheReceipt() {
        for (cancel in listOf(false,true)) {
            val permissions = Permissions(); val signal = CancellationSignal()
            if (cancel) permissions.takeAction = { signal.cancel() }
            val manager = BackupPersistentAccess(UUID.randomUUID(), Documents(), permissions) {}
            val review = manager.review(tree, flags, signal) {}
            val receipt = manager.accept(review, true, true, { true }, signal, { signal.throwIfCanceled() }, {}, { if (!cancel) throw IOException("lost outcome acknowledgement") })
            assertEquals(BackupPersistentAccess.Outcome.Uncertain, receipt.outcome)
            assertTrue(permissions.held.complete)
        }
    }
    @Test fun inFlightInspectionCannotResurrectAnInvalidatedPolicyEpoch() {
        val docs = Documents(); val permissions = Permissions(); val manager = BackupPersistentAccess(UUID.randomUUID(), docs, permissions) {}
        docs.inspectAction = { manager.invalidate() }
        unavailable { manager.review(tree, flags, CancellationSignal()) {} }
        assertEquals(0, permissions.taken)
    }
    @Test fun invalidationDuringProofOrConsentRecordingPreventsGrantEntry() {
        for (duringProof in listOf(false,true)) {
            val permissions = Permissions(); val manager = BackupPersistentAccess(UUID.randomUUID(), Documents(), permissions) {}
            val signal = CancellationSignal(); val review = manager.review(tree, flags, signal) {}
            unavailable { manager.accept(review, true, true, { if (duringProof) manager.invalidate(); true }, signal, {},
                { if (!duringProof) manager.invalidate() }, {}) }
            assertEquals(0, permissions.taken)
        }
    }
    @Test fun interruptionAfterGrantEntryPreservesBothTheFlagAndEffectReceipt() {
        val permissions = Permissions(); permissions.takeAction = { throw InterruptedException("injected") }
        val manager = BackupPersistentAccess(UUID.randomUUID(), Documents(), permissions) {}
        val signal = CancellationSignal(); val review = manager.review(tree, flags, signal) {}
        try {
            val receipt = manager.accept(review, true, true, { true }, signal, {}, {}, {})
            assertEquals(BackupPersistentAccess.Outcome.Uncertain, receipt.outcome)
            assertTrue(Thread.currentThread().isInterrupted); assertTrue(permissions.held.complete)
        } finally { Thread.interrupted() }
    }
    @Test fun foreignAndInvalidatedReviewsCannotAuthorisePersistence() {
        val permissions = Permissions(); val manager = BackupPersistentAccess(UUID.randomUUID(), Documents(), permissions) {}
        val other = BackupPersistentAccess(UUID.randomUUID(), Documents(), permissions) {}
        val signal = CancellationSignal(); val review = manager.review(tree, flags, signal) {}
        unavailable { other.accept(review, true, true, { true }, signal, {}, {}, {}) }
        val fresh = manager.review(tree, flags, signal) {}; manager.invalidate()
        unavailable { manager.accept(fresh, true, true, { true }, signal, {}, {}, {}) }
        assertEquals(0, permissions.taken)
    }
}
