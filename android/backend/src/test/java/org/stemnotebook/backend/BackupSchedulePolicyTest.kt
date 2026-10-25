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
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class BackupSchedulePolicyTest {
    @get:Rule val temporary = TemporaryFolder()
    private val library = UUID.randomUUID()
    private fun folder() = File(temporary.newFolder(), "policy")
    // Structural fixture only. Credential-vault tests separately use real AEAD.
    private fun credential(store: BackupPolicyStore, library: UUID = this.library, recovery: Boolean = false): BackupSecretContext {
        val context = BackupSecretContext(library, UUID.randomUUID(), if (recovery) BackupSecretContext.Purpose.RecoveryKey else BackupSecretContext.Purpose.SchedulePassword)
        store.beginCredential(context, if (recovery) UUID.randomUUID() else null, true, true)
        store.commitCredential(context, "STEMKEY1".toByteArray() + byteArrayOf(1) + ByteArray(30))
        return context
    }
    private fun arm(policy: BackupSchedulePolicy, key: BackupSecretContext, previous: BackupSchedulePolicy.Configuration? = null) =
        policy.arm(library, previous?.revision, BackupSchedulePolicy.Destination.LocalDirectory, "/explicit/private/backups", key.id, null, 123456L, true, false, true, maximumBytes = 1_000_000, destinationIdentity = "synthetic-directory-identity")
    private fun unavailable(action: () -> Unit) { try { action(); fail("Invalid schedule operation accepted") } catch (_: IOException) {} }
    @Test fun persistedSetupNeverRunsUntilExactRegistrationAcknowledges() {
        val folder = folder(); val store = BackupPolicyStore(folder); val key = credential(store); val policy = BackupSchedulePolicy(folder)
        val arming = arm(policy, key)
        val restarted = BackupSchedulePolicy(folder)
        assertEquals(arming, restarted.configuration(library)); assertEquals(BackupSchedulePolicy.State.Arming, arming.state)
        assertNull(restarted.start(library, arming.id, arming.revision))
        unavailable { restarted.registrationAcknowledged(library, UUID.randomUUID()) }
        val enabled = restarted.registrationAcknowledged(library, arming.revision)
        assertEquals(BackupSchedulePolicy.State.Enabled, enabled.state)
        assertEquals(enabled, BackupSchedulePolicy(folder).configuration(library))
        assertNotNull(restarted.start(library, enabled.id, enabled.revision))
    }
    @Test fun scheduleRequiresConsentReadySameLibraryPasswordAndExactCurrentRevision() {
        val folder = folder(); val store = BackupPolicyStore(folder); val policy = BackupSchedulePolicy(folder)
        val intent = BackupSecretContext(library, UUID.randomUUID(), BackupSecretContext.Purpose.SchedulePassword)
        store.beginCredential(intent, null, true, true)
        unavailable { arm(policy, intent) }
        unavailable { arm(policy, credential(store, UUID.randomUUID())) }
        unavailable { arm(policy, credential(store, recovery = true)) }
        val ready = credential(store)
        unavailable { policy.arm(library, null, BackupSchedulePolicy.Destination.LocalDirectory, "/explicit/backups", ready.id, null, 123456L, true, false, false, maximumBytes = 1_000_000, destinationIdentity = "synthetic-directory-identity") }
        assertNull(policy.configuration(library))
        val arming = arm(policy, ready)
        unavailable { arm(policy, ready) }
        assertEquals(arming, policy.configuration(library))
    }
    @Test fun disableRejectsStaleJobsAndRetainsActualRunFenceUntilCompletion() {
        val folder = folder(); val store = BackupPolicyStore(folder); val key = credential(store); val policy = BackupSchedulePolicy(folder)
        val arming = arm(policy, key); val enabled = policy.registrationAcknowledged(library, arming.revision)
        val run = policy.start(library, enabled.id, enabled.revision)!!
        assertNull(policy.start(library, enabled.id, enabled.revision))
        val disabled = policy.disable(library, enabled.revision)
        assertNotEquals(enabled.revision, disabled.revision); assertEquals(run.id, disabled.activeRun)
        assertNull(policy.start(library, enabled.id, enabled.revision))
        unavailable { arm(policy, key, disabled) }
        val done = policy.finish(library, run, BackupSchedulePolicy.Result.Cancelled)
        assertEquals(BackupSchedulePolicy.Result.Cancelled, done.result)
        val retired = policy.configuration(library)!!; assertNull(retired.activeRun); assertEquals(BackupSchedulePolicy.State.Disabled, retired.state)
        val replacement = arm(policy, key, retired)
        assertEquals(enabled.id, replacement.id); assertNotEquals(enabled.revision, replacement.revision)
        unavailable { policy.registrationAcknowledged(library, enabled.revision) }
        assertNull(policy.start(library, enabled.id, enabled.revision))
    }
    @Test fun durableCredentialRetirementAtomicallyDisablesReferencingSchedule() {
        val folder = folder(); val store = BackupPolicyStore(folder); val key = credential(store); val policy = BackupSchedulePolicy(folder)
        val arming = arm(policy, key); val enabled = policy.registrationAcknowledged(library, arming.revision)
        store.markRetired(key)
        val restarted = BackupSchedulePolicy(folder); val disabled = restarted.configuration(library)!!
        assertEquals(BackupSchedulePolicy.State.Disabled, disabled.state); assertNotEquals(enabled.revision, disabled.revision)
        assertNull(restarted.start(library, enabled.id, enabled.revision))
        unavailable { restarted.registrationAcknowledged(library, enabled.revision) }
        unavailable { arm(restarted, key, disabled) }
        assertEquals(BackupPolicyStore.CredentialState.Retired, BackupPolicyStore(folder).credential(key)!!.state)
    }
    @Test fun credentialRetirementDuringRunKeepsFenceAndNeverReenablesOnCompletion() {
        val folder = folder(); val store = BackupPolicyStore(folder); val key = credential(store); val policy = BackupSchedulePolicy(folder)
        val arming = arm(policy, key); val enabled = policy.registrationAcknowledged(library, arming.revision)
        val run = policy.start(library, enabled.id, enabled.revision)!!
        store.markRetired(key)
        val disabled = policy.configuration(library)!!
        assertEquals(run.id, disabled.activeRun); assertEquals(BackupSchedulePolicy.State.Disabled, disabled.state)
        assertNull(policy.start(library, enabled.id, enabled.revision))
        val fresh = credential(store)
        unavailable { arm(policy, fresh, disabled) }
        policy.finish(library, run, BackupSchedulePolicy.Result.CredentialUnavailable)
        val retired = policy.configuration(library)!!
        assertEquals(BackupSchedulePolicy.State.Disabled, retired.state); assertEquals(disabled.revision, retired.revision)
        assertNull(retired.activeRun)
        assertEquals(BackupSchedulePolicy.State.Arming, arm(policy, fresh, retired).state)
    }
    @Test fun retirementRollbackCannotDisableScheduleWithoutRetiringItsCredential() {
        val folder = folder(); val store = BackupPolicyStore(folder); val key = credential(store); val policy = BackupSchedulePolicy(folder)
        val arming = arm(policy, key); val enabled = policy.registrationAcknowledged(library, arming.revision)
        val failing = BackupPolicyStore(folder) { if (it == "policy-before-commit") throw IOException("injected") }
        unavailable { failing.markRetired(key) }
        assertEquals(enabled, policy.configuration(library))
        assertEquals(BackupPolicyStore.CredentialState.Ready, store.credential(key)!!.state)
    }
    @Test fun lostRegistrationAndRunAcknowledgementsAreDiscoverableWithoutDuplicateRuns() {
        val folder = folder(); val store = BackupPolicyStore(folder); val key = credential(store); val normal = BackupSchedulePolicy(folder)
        val arming = arm(normal, key)
        val failing = BackupSchedulePolicy(folder) { if (it == "schedule-after-commit") throw IOException("lost") }
        unavailable { failing.registrationAcknowledged(library, arming.revision) }
        val enabled = BackupSchedulePolicy(folder).configuration(library)!!
        assertEquals(BackupSchedulePolicy.State.Enabled, enabled.state)
        unavailable { failing.start(library, enabled.id, enabled.revision) }
        val restarted = BackupSchedulePolicy(folder)
        val run = restarted.runs(library).single()
        assertEquals(run.id, restarted.configuration(library)!!.activeRun)
        assertNull(restarted.start(library, enabled.id, enabled.revision))
        val interrupted = restarted.reconcileInterrupted(library)!!
        assertEquals(BackupSchedulePolicy.Result.Interrupted, interrupted.result)
        assertNull(restarted.reconcileInterrupted(library)); assertNull(restarted.configuration(library)!!.activeRun)
        unavailable { restarted.finish(library, run, BackupSchedulePolicy.Result.Published, 1) }
    }
    @Test fun providerPolicyNeedsRecordedCompleteConsentForExactLibraryAndTree() {
        val folder = folder(); val store = BackupPolicyStore(folder); val key = credential(store); val policy = BackupSchedulePolicy(folder)
        val decision = BackupPersistentAccess.Decision(UUID.randomUUID(), library, "content://example/tree/directory", "directory", false, false, 123)
        store.recordPermissionDecision(decision)
        fun attempt(tree: String = decision.tree) = policy.arm(library, null, BackupSchedulePolicy.Destination.ProviderTree, tree, key.id, decision.id, 123456L, true, true, true, maximumBytes = 1_000_000, destinationIdentity = "synthetic-directory-identity")
        unavailable { attempt() }
        store.recordPermissionOutcome(BackupPersistentAccess.Receipt(decision, BackupPersistentAccess.Outcome.Uncertain, true, true))
        unavailable { attempt() }
        val fresh = decision.copy(id = UUID.randomUUID())
        store.recordPermissionDecision(fresh)
        store.recordPermissionOutcome(BackupPersistentAccess.Receipt(fresh, BackupPersistentAccess.Outcome.AlreadyHeld, true, true))
        unavailable { policy.arm(library, null, BackupSchedulePolicy.Destination.ProviderTree, "content://example/tree/other", key.id, fresh.id, 123456L, true, true, true, maximumBytes = 1_000_000, destinationIdentity = "synthetic-directory-identity") }
        val config = policy.arm(library, null, BackupSchedulePolicy.Destination.ProviderTree, fresh.tree, key.id, fresh.id, 123456L, true, true, true, maximumBytes = 1_000_000, destinationIdentity = "synthetic-directory-identity")
        assertEquals(fresh.id, config.permission); assertTrue(config.sourceAssets)
    }
    @Test fun publicationOutcomeIsImmutableAndDoesNotReenableDisabledPolicy() {
        val folder = folder(); val store = BackupPolicyStore(folder); val key = credential(store); val policy = BackupSchedulePolicy(folder)
        val arming = arm(policy, key); val enabled = policy.registrationAcknowledged(library, arming.revision)
        val run = policy.start(library, enabled.id, enabled.revision)!!
        unavailable { policy.finish(library, run, BackupSchedulePolicy.Result.Uncertain, 1) }
        val disabled = policy.disable(library, enabled.revision)
        val done = policy.finish(library, run, BackupSchedulePolicy.Result.Published, 42)
        assertEquals(42L, done.publicationSequence); assertNotNull(done.finishedAtMillis)
        unavailable { policy.finish(library, run, BackupSchedulePolicy.Result.Failed) }
        assertEquals(done, BackupSchedulePolicy(folder).runs(library).single())
        assertEquals(disabled.revision, policy.configuration(library)!!.revision)
        assertEquals(BackupSchedulePolicy.State.Disabled, policy.configuration(library)!!.state)
    }
    @Test fun twoControllersCannotAdmitTheSameActiveScheduleTwice() {
        val folder = folder(); val store = BackupPolicyStore(folder); val key = credential(store); val policy = BackupSchedulePolicy(folder)
        val arming = arm(policy, key); val enabled = policy.registrationAcknowledged(library, arming.revision)
        val start = CountDownLatch(1); val ready = CountDownLatch(2)
        val results = java.util.Collections.synchronizedList(mutableListOf<BackupSchedulePolicy.Run?>())
        val errors = java.util.Collections.synchronizedList(mutableListOf<Throwable>())
        val threads = (0..1).map { Thread {
            try { val other = BackupSchedulePolicy(folder); ready.countDown(); check(start.await(30, TimeUnit.SECONDS)); results.add(other.start(library, enabled.id, enabled.revision)) }
            catch (error: Throwable) { errors.add(error) }
        }.apply { start() } }
        assertTrue(ready.await(30, TimeUnit.SECONDS)); start.countDown(); threads.forEach { it.join(30000); assertFalse(it.isAlive) }
        assertTrue(errors.toString(), errors.isEmpty()); assertEquals(1, results.count { it != null }); assertEquals(1, policy.runs(library).size)
    }
}
