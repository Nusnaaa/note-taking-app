package org.stemnotebook.backend

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.CancellationSignal
import android.system.Os
import java.io.Closeable
import java.io.File
import java.io.IOException
import java.nio.file.Files
import java.nio.file.LinkOption
import java.util.UUID
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.CompletableFuture
import java.util.concurrent.ExecutionException
import java.util.concurrent.ScheduledThreadPoolExecutor
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/** Optional-worker scheduling controller. No schedule, OS grant or credential is
 * created merely by constructing/opening the backend. */
class NotebookSchedules internal constructor(
    context: Context, directory: File, private val backups: NotebookBackups,
    private val library: () -> UUID, private val authority: () -> Unit,
    private val jobs: BackupJobScheduler = BackupJobScheduler.platform(context),
    private val documents: BackupDocuments = BackupDocuments.platform(context),
    private val permissions: BackupPersistentPermissions = BackupPersistentPermissions.platform(context),
    keys: BackupSecretKeys = AndroidBackupSecretKeys(context),
) : Closeable {
    class Unavailable : IOException("Scheduled backup operation unavailable")
    sealed class Target {
        data class Directory(val folder: File) : Target()
        data class Provider(val tree: Uri, val pickerFlags: Int) : Target()
    }
    data class Options(val target: Target, val intervalMillis: Long, val maximumBytes: Long,
        val originalInk: Boolean = true, val sourceAssets: Boolean = false)
    enum class State { Arming, Enabled, Disabled }
    enum class Registration { NotAttempted, Registered, Rejected, Uncertain }
    enum class Publication { Published, ProviderVerified, Uncertain }
    data class Configuration(val id: UUID, val library: UUID, val revision: UUID, val state: State,
        val destination: String, val intervalMillis: Long, val maximumBytes: Long?, val activeRun: UUID?)
    data class EnableReceipt(val backup: UUID, val initialPublication: Publication?, val catalogueRecorded: Boolean,
        val configuration: Configuration?, val credential: UUID?, val registration: Registration, val needsAttention: Boolean,
        val persistentAccessOutcome: String? = null, val permissionDecision: UUID? = null)
    data class DisableReceipt(val configuration: Configuration, val jobCancellationAcknowledged: Boolean,
        val credentialDeletionAcknowledged: Boolean, val workRetirementAcknowledged: Boolean)
    data class RunReceipt(val run: UUID?, val result: String, val backup: UUID?, val catalogueRecorded: Boolean)
    data class StoredKey(val id: UUID, val backup: UUID, val state: String)

    private val lock = Any()
    private var closed = false
    private var workBusy = false
    private var controlBusy = false
    private var prepared: Prepared? = null
    private var running: Operation<*>? = null
    private val operations = mutableSetOf<Operation<*>>()
    private val store = BackupPolicyStore(directory)
    private val policies = BackupSchedulePolicy(directory)
    private val vault = BackupCredentialVault(store, BackupSecretProtector(keys))
    private fun executor(name: String) = ThreadPoolExecutor(1, 1, 0, TimeUnit.MILLISECONDS, ArrayBlockingQueue<Runnable>(2),
        { action -> Thread(action, name).apply { isDaemon = true } }, ThreadPoolExecutor.AbortPolicy())
    private val worker = executor("stem-schedule-work")
    private val control = executor("stem-schedule-control")
    private val cancellations = executor("stem-schedule-cancel")
    private val expiry = ScheduledThreadPoolExecutor(1) { action -> Thread(action, "stem-schedule-expiry").apply { isDaemon = true } }
        .apply { removeOnCancelPolicy = true }

    class Pending<T : Any> internal constructor(private val operation: Operation<T>) : Closeable {
        private val taken = AtomicBoolean()
        fun await(): T {
            check(taken.compareAndSet(false, true))
            return try {
                val result = operation.future.get()
                operation.claim(result)
                result
            } catch (error: InterruptedException) {
                operation.cancel(); Thread.currentThread().interrupt(); throw error
            } catch (error: ExecutionException) { throw error.cause ?: Unavailable() }
        }
        fun cancel(): Boolean = operation.cancel()
        /** Includes actual action, native/provider/key cleanup and cancellation. */
        fun awaitRetired() { try { operation.future.join() } catch (_: Exception) { } }
        override fun close() { cancel() }
    }
    internal inner class Operation<T : Any>(val preserve: Boolean, private val dispose: (T) -> Unit) {
        val future = CompletableFuture<T>()
        private var cancelled = false
        private var hook: Closeable? = null
        private var cancelling: CompletableFuture<Void>? = null
        private var result: T? = null
        private var claimed = false
        fun check() { synchronized(this) { if (cancelled) throw java.util.concurrent.CancellationException("Scheduled backup cancelled") }; synchronized(lock) { check(!closed) } }
        fun cancel(): Boolean {
            val cleanup: T?
            synchronized(this) {
                if (cancelled || claimed || (preserve && future.isDone)) return false
                cancelled = true
                hook?.let { cancelHook(it) }
                cleanup = if (!claimed && !preserve) result.also { result = null } else null
            }
            cleanup?.let(dispose)
            return true
        }
        private fun cancelHook(value: Closeable) {
            if (cancelling != null) return
            val done = CompletableFuture<Void>(); cancelling = done
            cancellations.execute { try { value.close() } catch (_: Exception) { } finally { done.complete(null) } }
        }
        fun <V : Any> observe(pending: NotebookBackups.Pending<V>): V {
            synchronized(this) { hook = pending; if (cancelled) cancelHook(pending) }
            return try { pending.await() } finally {
                val wait = synchronized(this) { hook = null; cancelling }
                wait?.join()
            }
        }
        fun signal(): CancellationSignal {
            val signal = CancellationSignal()
            synchronized(this) { hook = Closeable { signal.cancel() }; if (cancelled) cancelHook(checkNotNull(hook)) }
            return signal
        }
        fun unbind() { val wait = synchronized(this) { hook = null; cancelling }; wait?.join() }
        fun complete(value: T) {
            val abandon = synchronized(this) { if (cancelled && !preserve) true else { result = value; false } }
            if (abandon) { dispose(value); future.completeExceptionally(java.util.concurrent.CancellationException("Scheduled backup cancelled")) }
            else future.complete(value)
        }
        fun claim(value: T) {
            synchronized(this) {
                if (cancelled && !preserve) throw java.util.concurrent.CancellationException("Scheduled backup cancelled")
                check(result === value); claimed = true; result = null
            }
        }
    }
    private fun <T : Any> submit(isControl: Boolean = false, preserve: Boolean = false, dispose: (T) -> Unit = {}, action: (Operation<T>) -> T): Pending<T> {
        val op = Operation(preserve, dispose)
        synchronized(lock) {
            check(!closed && !(if (isControl) controlBusy else workBusy))
            if (isControl) controlBusy = true else workBusy = true
            operations.add(op)
            try {
                (if (isControl) control else worker).execute {
                    var value: T? = null; var error: Throwable? = null
                    try { op.check(); value = action(op) } catch (problem: Throwable) { if (problem is InterruptedException) Thread.currentThread().interrupt(); error = problem }
                    try { op.unbind() } catch (problem: Throwable) { if (error == null) error = problem }
                    synchronized(lock) { operations.remove(op); if (running === op) running = null; if (isControl) controlBusy = false else workBusy = false }
                    if (error == null) op.complete(checkNotNull(value)) else op.future.completeExceptionally(
                        if (error is java.util.concurrent.CancellationException || error is android.os.OperationCanceledException || error is InterruptedException) java.util.concurrent.CancellationException("Scheduled backup cancelled") else Unavailable())
                }
            } catch (error: Exception) { operations.remove(op); if (isControl) controlBusy = false else workBusy = false; throw error }
        }
        return Pending(op)
    }

    inner class Prepared internal constructor(internal val artifact: NotebookBackups.Prepared, private var secret: CharArray?,
        internal val options: Options, internal val identity: String, internal val prior: BackupSchedulePolicy.Configuration?,
        internal val provider: BackupPersistentAccess?, internal val providerReview: BackupPersistentAccess.Review?,
        internal val ownerLibrary: UUID) : Closeable {
        internal var live = true
        private var keyOffered = false
        internal var timer: java.util.concurrent.ScheduledFuture<*>? = null
        val label: String get() = synchronized(lock) { check(live && prepared === this); providerReview?.label ?: (options.target as Target.Directory).folder.name }
        val storesDevicePasswordAndRecoveryKeys: Boolean get() = true
        val requiresProtectedContentConsent: Boolean get() = artifact.authority.protectedContent
        fun copyRecoveryKey(): CharArray = synchronized(lock) { check(live && prepared === this); artifact.copyRecoveryKey().also { keyOffered = true } }
        internal fun take(): CharArray = synchronized(lock) {
            check(live && prepared === this && keyOffered)
            live = false; prepared = null; timer?.cancel(false)
            checkNotNull(secret).also { secret = null }
        }
        override fun close() {
            val retire = synchronized(lock) {
                if (!live) false else { live = false; secret?.fill('\u0000'); secret = null; if (prepared === this) prepared = null; timer?.cancel(false); true }
            }
            if (retire) { provider?.invalidate(); providerReview?.close(); artifact.close() }
        }
    }
    fun minimumPeriodMillis(): Long = jobs.minimumPeriodMillis()
    fun status(): Pending<ConfigurationResult> = submit { op -> op.check(); authority(); ConfigurationResult(policies.configuration(library())?.public(), jobs.supportsCurrentRoot()) }
    data class ConfigurationResult(val configuration: Configuration?, val executionRootSupported: Boolean)

    /** Initial encryption validates native password policy and offers an actual
     * independent key before the caller can confirm scheduling/device storage. */
    fun prepare(options: Options, password: CharArray, proof: NativeBridge.Verification, lifetimeMillis: Long = 120_000, allowProtectedContent: Boolean = false): Pending<Prepared> {
        require(password.size <= 1024 && options.maximumBytes > 0 && options.intervalMillis >= jobs.minimumPeriodMillis())
        require(lifetimeMillis in 1..3_600_000)
        val secret = password.clone()
        return try { submit(dispose = { it.close() }) { op ->
            var artifact: NotebookBackups.Prepared? = null; var manager: BackupPersistentAccess? = null; var review: BackupPersistentAccess.Review? = null
            var handedOff = false
            try {
                synchronized(lock) { check(prepared == null) }
                check(jobs.supportsCurrentRoot())
                authority(); val owner = library(); val prior = policies.configuration(owner)
                check(prior?.activeRun == null)
                val identity = when (val target = options.target) {
                    is Target.Directory -> directoryIdentity(target.folder)
                    is Target.Provider -> {
                        manager = BackupPersistentAccess(owner, documents, permissions, authority)
                        review = manager.review(target.tree, target.pickerFlags, op.signal(), op::check)
                        op.unbind(); android.provider.DocumentsContract.getTreeDocumentId(target.tree)
                    }
                }
                op.check()
                // This is a fresh, explicitly consented initial backup proof,
                // never the verifier used for a later background run. It grants
                // the exact native plan only and does not unlock ordinary reads.
                val nativeReview = op.observe(if (!allowProtectedContent) backups.review(options.originalInk, options.sourceAssets)
                    else backups.reviewDelegated(options.originalInk, options.sourceAssets, NativeBridge.BackupAuthorization { challenge, version, actualLibrary, _, _ ->
                        version == 1 && actualLibrary == owner.toString() && proof.verify(challenge)
                    }))
                artifact = try { op.observe(backups.create(nativeReview, secret, proof, true, options.maximumBytes)) } finally { nativeReview.close() }
                op.check(); authority(); check(library() == owner)
                val result = Prepared(artifact, secret, options, identity, prior, manager, review, owner)
                synchronized(lock) { op.check(); check(prepared == null); prepared = result; result.timer = expiry.schedule({ result.close() }, lifetimeMillis, TimeUnit.MILLISECONDS) }
                handedOff = true; result
            } finally {
                if (!handedOff) { secret.fill('\u0000'); review?.close(); manager?.invalidate(); artifact?.close(); backups.awaitRetired() }
            }
        } } catch (error: Exception) { secret.fill('\u0000'); throw error }
    }
    fun enable(review: Prepared, proof: NativeBridge.Verification, confirmedScheduling: Boolean,
        allowDevicePasswordAndRecoveryKeyStorage: Boolean, allowPersistentProviderAccess: Boolean,
        independentRecoveryAcknowledged: Boolean, allowProtectedContentDelegation: Boolean = false): Pending<EnableReceipt> = submit(preserve = true) { op ->
        require(confirmedScheduling && allowDevicePasswordAndRecoveryKeyStorage && independentRecoveryAcknowledged)
        require(!review.requiresProtectedContentConsent || allowProtectedContentDelegation)
        authority(); check(library() == review.ownerLibrary)
        check(proof.verify(UUID.randomUUID().toString())); op.check(); authority()
        check(policies.configuration(review.ownerLibrary)?.revision == review.prior?.revision)
        val secret = review.take()
        var initial: Attempt? = null; var config: BackupSchedulePolicy.Configuration? = null; var credential: UUID? = null
        var registration = Registration.NotAttempted
        var grant: BackupPersistentAccess.Receipt? = null
        val receipt = try {
            val permission = if (review.provider != null) {
                val receipt = review.provider.accept(checkNotNull(review.providerReview), true, allowPersistentProviderAccess,
                    proof::verify, op.signal(), op::check, store::recordPermissionDecision, store::recordPermissionOutcome)
                grant = receipt
                op.unbind(); check(receipt.outcome != BackupPersistentAccess.Outcome.Uncertain); receipt.decision.id
            } else null
            op.check(); check(policies.configuration(review.ownerLibrary)?.revision == review.prior?.revision)
            checkTarget(review.options.target, review.identity)
            initial = publish(op, review.artifact, review.options.target, review.identity)
            if (initial.publication == Publication.Uncertain) throw Unavailable()
            authority(); op.check(); check(library() == review.ownerLibrary)
            val password = BackupSecretContext(review.ownerLibrary, UUID.randomUUID(), BackupSecretContext.Purpose.SchedulePassword)
            credential = password.id
            vault.store(password, null, secret, true, true)
            storeRecovery(review.ownerLibrary, review.artifact)
            val kind = when (review.options.target) { is Target.Directory -> BackupSchedulePolicy.Destination.LocalDirectory; is Target.Provider -> BackupSchedulePolicy.Destination.ProviderTree }
            val location = when (val target = review.options.target) { is Target.Directory -> target.folder.absolutePath; is Target.Provider -> target.tree.toString() }
            val delegation = BackupSecretContext(review.ownerLibrary, UUID.randomUUID(), BackupSecretContext.Purpose.BackupDelegation)
            store.beginCredential(delegation, null, true, true)
            config = policies.arm(review.ownerLibrary, review.prior?.revision, kind, location, password.id, permission,
                review.options.intervalMillis, review.options.originalInk, review.options.sourceAssets, true, review.options.maximumBytes, review.identity, delegation.id)
            val approval = approvalPayload(config, delegation.id, review.artifact.authority)
            try { vault.sealIntent(delegation, approval) } finally { approval.fill('\u0000') }
            registration = Registration.valueOf(jobs.register(config).name)
            if (registration == Registration.Registered) config = policies.registrationAcknowledged(review.ownerLibrary, config.revision)
            val retiredPrior = if (review.prior != null) try {
                retireConfigurationCredentials(review.prior); true
            } catch (_: Exception) { false } else true
            EnableReceipt(review.artifact.id, initial.publication, initial.catalogueRecorded, config.public(), credential, registration, registration != Registration.Registered || !initial.catalogueRecorded || backups.needsAttention || !retiredPrior,
                grant?.outcome?.name, grant?.decision?.id)
        } catch (error: Exception) {
            if (error is InterruptedException) Thread.currentThread().interrupt()
            if (initial == null && grant == null) throw error
            EnableReceipt(review.artifact.id, initial?.publication, initial?.catalogueRecorded ?: false, config?.public(), credential, registration, true,
                grant?.outcome?.name, grant?.decision?.id)
        } finally {
            secret.fill('\u0000'); review.providerReview?.close(); review.provider?.invalidate(); review.artifact.close(); backups.awaitRetired()
        }
        receipt.copy(needsAttention = receipt.needsAttention || backups.needsAttention)
    }
    fun disable(expected: UUID, proof: NativeBridge.Verification, confirmed: Boolean): Pending<DisableReceipt> = submit(isControl = true, preserve = true) { op ->
        require(confirmed); authority(); check(proof.verify(UUID.randomUUID().toString())); op.check(); authority()
        val owner = library(); val prior = policies.configuration(owner) ?: throw Unavailable(); check(prior.revision == expected)
        val disabled = policies.disable(owner, expected)
        val abandoned = synchronized(lock) { prepared to operations.filter { it !== op } }
        abandoned.first?.close(); abandoned.second.forEach { it.cancel() }
        val cancelled = try { jobs.cancel(BackupJobScheduler.Token(prior.id, prior.revision)) in setOf(BackupJobScheduler.Cancellation.Cancelled, BackupJobScheduler.Cancellation.Absent) } catch (_: Exception) { false }
        val deleted = try { retireConfigurationCredentials(prior); true } catch (_: Exception) { false }
        abandoned.second.forEach { try { it.future.join() } catch (_: Exception) { } }
        val current = try { policies.configuration(owner) ?: disabled } catch (_: Exception) { disabled }
        DisableReceipt(current.public(), cancelled, deleted, current.activeRun == null)
    }
    /** Explicit owner reconciliation, never automatic startup enablement. */
    fun reconcileRegistration(expected: UUID, proof: NativeBridge.Verification, confirmed: Boolean): Pending<Configuration> = submit { op ->
        require(confirmed); authority(); check(proof.verify(UUID.randomUUID().toString())); op.check(); authority()
        val owner = library(); val config = policies.configuration(owner) ?: throw Unavailable()
        check(config.revision == expected && config.state == BackupSchedulePolicy.State.Arming)
        val token = BackupJobScheduler.Token(config.id, config.revision)
        check(jobs.pending() == token)
        policies.registrationAcknowledged(owner, expected).public()
    }
    /** Owner-reviewed repair of an interrupted run record. This controller's
     * single work worker cannot overlap an earlier scheduled producer. */
    fun reconcileInterrupted(expected: UUID, proof: NativeBridge.Verification, confirmed: Boolean): Pending<RunReceipt> = submit(preserve = true) { op ->
        require(confirmed); authority(); check(proof.verify(UUID.randomUUID().toString())); op.check(); authority()
        val owner = library(); val config = policies.configuration(owner) ?: throw Unavailable()
        check(config.revision == expected && synchronized(lock) { running == null })
        backups.awaitRetired()
        val run = policies.reconcileInterrupted(owner)
        RunReceipt(run?.id, run?.result?.name ?: "NothingToReconcile", null, false)
    }
    private data class Attempt(val publication: Publication, val catalogueRecorded: Boolean)
    private fun publish(op: Operation<*>, artifact: NotebookBackups.Prepared, target: Target, identity: String): Attempt {
        checkTarget(target, identity)
        val result = when (target) {
            is Target.Directory -> {
                val receipt = op.observe(artifact.publish(File(target.folder, "backup-${artifact.id}.stemenc"), true))
                Attempt(Publication.valueOf(receipt.outcome.name), receipt.catalogueRecorded)
            }
            is Target.Provider -> {
                val selection = op.observe(artifact.reviewProvider(target.tree, Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION))
                try { val receipt = op.observe(selection.publish(true)); Attempt(Publication.valueOf(receipt.outcome.name), receipt.catalogueRecorded) } finally { selection.close() }
            }
        }
        return try { checkTarget(target, identity); result } catch (_: Exception) { result.copy(publication = Publication.Uncertain) }
    }
    /** The encrypted payload binds review metadata to this exact policy and
     * credential. Plain SQLite scope metadata cannot manufacture approval. */
    private fun approvalPayload(config: BackupSchedulePolicy.Configuration, grant: UUID, authority: NativeBridge.BackupAuthority): CharArray =
        approvalPayload(config, grant, authority.version, authority.library, authority.scopeDigest, authority.protectedContent)
    private fun approvalPayload(config: BackupSchedulePolicy.Configuration, grant: UUID, version: Int, actualLibrary: String, digest: String, protected: Boolean): CharArray {
        require(version == 1 && actualLibrary == config.library.toString() && digest.length == 64 && digest.all { it in '0'..'9' || it in 'a'..'f' })
        val policy = org.json.JSONArray().put(config.kind.name).put(config.destination).put(config.credential.toString())
            .put(config.permission?.toString()).put(config.intervalMillis).put(config.originalInk).put(config.sourceAssets)
            .put(config.maximumBytes).put(config.destinationIdentity).toString().toByteArray(Charsets.UTF_8)
        val binding = java.security.MessageDigest.getInstance("SHA-256").digest(policy).joinToString("") { "%02x".format(it.toInt() and 255) }
        return "STEM-BACKUP-DELEGATION-1:$version:$actualLibrary:${config.id}:${config.revision}:$grant:$digest:$protected:$binding".toCharArray()
    }
    private fun retireConfigurationCredentials(config: BackupSchedulePolicy.Configuration) {
        // Attempt both retirements even when one device-key deletion fails.
        var failure: Exception? = null
        val contexts = listOfNotNull(BackupSecretContext(config.library, config.credential, BackupSecretContext.Purpose.SchedulePassword),
            config.authority?.let { BackupSecretContext(config.library, it, BackupSecretContext.Purpose.BackupDelegation) })
        for (context in contexts) try { vault.retire(context).join() } catch (error: Exception) { if (failure == null) failure = error }
        failure?.let { throw it }
    }
    private fun storeRecovery(owner: UUID, artifact: NotebookBackups.Prepared) {
        val key = artifact.copyRecoveryKey()
        try { vault.store(BackupSecretContext(owner, UUID.randomUUID(), BackupSecretContext.Purpose.RecoveryKey), artifact.id, key, true, true) }
        finally { key.fill('\u0000') }
    }
    internal fun runDue(token: BackupJobScheduler.Token): Pending<RunReceipt> = submit(preserve = true) { op ->
        synchronized(lock) { if (prepared != null) return@submit RunReceipt(null, "DeferredForReview", null, false) }
        val owner = library(); val config = policies.configuration(owner)
        if (config == null || config.id != token.schedule || config.revision != token.revision || config.state != BackupSchedulePolicy.State.Enabled) return@submit RunReceipt(null, "Ignored", null, false)
        val run = policies.start(owner, token.schedule, token.revision) ?: return@submit RunReceipt(null, "Ignored", null, false)
        synchronized(lock) { running = op }
        var result = BackupSchedulePolicy.Result.Failed; var attempt: Attempt? = null; var backup: UUID? = null
        try {
            val target = when (config.kind) {
                BackupSchedulePolicy.Destination.LocalDirectory -> Target.Directory(File(config.destination))
                BackupSchedulePolicy.Destination.ProviderTree -> Target.Provider(Uri.parse(config.destination), Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
            }
            if (target is Target.Provider) check(permissions.current(target.tree).complete)
            if (config.authority == null) authority()
            op.check(); checkTarget(target, checkNotNull(config.destinationIdentity))
            val outcome = vault.use(BackupSecretContext(owner, config.credential, BackupSecretContext.Purpose.SchedulePassword)) { secret ->
                // Preserve operation errors separately from a device-key error.
                var artifact: NotebookBackups.Prepared? = null
                try {
                    val review = op.observe(if (config.authority == null) backups.review(config.originalInk, config.sourceAssets)
                        else backups.reviewDelegated(config.originalInk, config.sourceAssets, NativeBridge.BackupAuthorization { _, version, actualLibrary, digest, protected ->
                            try {
                                op.check()
                                val grantId = checkNotNull(config.authority)
                                vault.use(BackupSecretContext(owner, grantId, BackupSecretContext.Purpose.BackupDelegation)) { approved ->
                                    val current = policies.configuration(owner)
                                    if (current?.id != config.id || current.revision != config.revision || current.state != BackupSchedulePolicy.State.Enabled || current.authority != grantId) false
                                    else {
                                        val expected = approvalPayload(config, grantId, version, actualLibrary, digest, protected)
                                        try { op.check(); approved.contentEquals(expected) } finally { expected.fill('\u0000') }
                                    }
                                }
                            } catch (_: Exception) { false }
                        }))
                    artifact = try { op.observe(backups.create(review, secret, NativeBridge.Verification { false }, true, checkNotNull(config.maximumBytes))) } finally { review.close() }
                    backup = artifact.id
                    op.check()
                    storeRecovery(owner, artifact)
                    attempt = publish(op, artifact, target, config.destinationIdentity)
                    Result.success(Unit)
                } catch (error: Exception) { Result.failure<Unit>(error) }
                finally { artifact?.close(); backups.awaitRetired() }
            }
            outcome.getOrThrow()
            result = when (attempt?.publication) { Publication.Published -> BackupSchedulePolicy.Result.Published; Publication.ProviderVerified -> BackupSchedulePolicy.Result.ProviderVerified; else -> BackupSchedulePolicy.Result.Uncertain }
        } catch (error: Exception) {
            result = when {
                attempt != null -> when (attempt?.publication) { Publication.Published -> BackupSchedulePolicy.Result.Published; Publication.ProviderVerified -> BackupSchedulePolicy.Result.ProviderVerified; else -> BackupSchedulePolicy.Result.Uncertain }
                error is BackupSecretUnavailable -> BackupSchedulePolicy.Result.CredentialUnavailable
                error is java.util.concurrent.CancellationException || error is android.os.OperationCanceledException -> BackupSchedulePolicy.Result.Cancelled
                else -> BackupSchedulePolicy.Result.Failed
            }
        } finally { backups.awaitRetired() }
        val publicationSequence = if (attempt?.catalogueRecorded == true && attempt?.publication != Publication.Uncertain) try {
            op.observe(backups.status()).lastPublication?.takeIf { it.id == backup.toString() }?.sequence
        } catch (_: Exception) { null } else null
        policies.finish(owner, run, result, publicationSequence)
        RunReceipt(run.id, result.name, backup, attempt?.catalogueRecorded ?: false)
    }
    data class StoredKeyPage(val keys: List<StoredKey>, val next: UUID?)
    fun storedRecoveryKeys(after: UUID? = null, limit: Int = 50): Pending<StoredKeyPage> = submit { op ->
        op.check(); authority(); val owner = library(); val rows = store.credentials(owner, after, limit)
        StoredKeyPage(rows.filter { it.context.purpose == BackupSecretContext.Purpose.RecoveryKey }.map { StoredKey(it.context.id, checkNotNull(it.backup), it.state.name) },
            if (rows.size == limit) rows.last().context.id else null)
    }
    data class StoredCredential(val id: UUID, val purpose: String, val state: String, val backup: UUID?)
    data class CredentialPage(val credentials: List<StoredCredential>, val next: UUID?)
    fun storedCredentials(after: UUID? = null, limit: Int = 50): Pending<CredentialPage> = submit { op ->
        op.check(); authority(); val rows = store.credentials(library(), after, limit)
        CredentialPage(rows.map { StoredCredential(it.context.id, it.context.purpose.name, it.state.name, it.backup) },
            if (rows.size == limit) rows.last().context.id else null)
    }
    /** No password reveal. Referenced Arming/Enabled or active-run credentials
     * need deliberate policy disablement first. Fresh setup always uses new IDs. */
    fun retireUnusedDelegation(id: UUID, proof: NativeBridge.Verification, confirmed: Boolean): Pending<DeletionReceipt> = submit(isControl = true, preserve = true) { op ->
        require(confirmed); authority(); check(proof.verify(UUID.randomUUID().toString())); op.check(); authority()
        val owner = library(); val config = policies.configuration(owner)
        check(config?.authority != id || (config.state == BackupSchedulePolicy.State.Disabled && config.activeRun == null))
        val context = BackupSecretContext(owner, id, BackupSecretContext.Purpose.BackupDelegation)
        check(store.credential(context) != null)
        try { vault.retire(context).join(); DeletionReceipt(true) } catch (_: Exception) { DeletionReceipt(false) }
    }
    fun retireUnusedPassword(id: UUID, proof: NativeBridge.Verification, confirmed: Boolean): Pending<DeletionReceipt> = submit(isControl = true, preserve = true) { op ->
        require(confirmed); authority(); check(proof.verify(UUID.randomUUID().toString())); op.check(); authority()
        val owner = library(); val config = policies.configuration(owner)
        check(config?.credential != id || (config.state == BackupSchedulePolicy.State.Disabled && config.activeRun == null))
        val context = BackupSecretContext(owner, id, BackupSecretContext.Purpose.SchedulePassword)
        check(store.credential(context) != null)
        try { vault.retire(context).join(); DeletionReceipt(true) } catch (_: Exception) { DeletionReceipt(false) }
    }
    fun revealRecoveryKey(id: UUID, proof: NativeBridge.Verification, confirmed: Boolean): Pending<CharArray> = submit(dispose = { it.fill('\u0000') }) { op ->
        require(confirmed); authority(); check(proof.verify(UUID.randomUUID().toString())); op.check(); authority()
        vault.use(BackupSecretContext(library(), id, BackupSecretContext.Purpose.RecoveryKey)) { it.clone() }
    }
    fun deleteRecoveryKey(id: UUID, proof: NativeBridge.Verification, confirmed: Boolean): Pending<DeletionReceipt> = submit(isControl = true, preserve = true) { op ->
        require(confirmed); authority(); check(proof.verify(UUID.randomUUID().toString())); op.check(); authority()
        val context = BackupSecretContext(library(), id, BackupSecretContext.Purpose.RecoveryKey)
        check(store.credential(context)?.backup != null)
        try { vault.retire(context).join(); DeletionReceipt(true) } catch (_: Exception) { DeletionReceipt(false) }
    }
    data class DeletionReceipt(val acknowledged: Boolean)
    private fun BackupSchedulePolicy.Configuration.public() = Configuration(id, library, revision, State.valueOf(state.name), destination, intervalMillis, maximumBytes, activeRun)
    private fun checkTarget(target: Target, identity: String) {
        when (target) {
            is Target.Directory -> check(directoryIdentity(target.folder) == identity)
            is Target.Provider -> check(android.provider.DocumentsContract.getTreeDocumentId(target.tree) == identity)
        }
    }
    private fun directoryIdentity(folder: File): String {
        require(folder.isAbsolute && folder.path.length <= 4096 && Files.isDirectory(folder.toPath(), LinkOption.NOFOLLOW_LINKS) && !Files.isSymbolicLink(folder.toPath()))
        return try {
            val a = Files.readAttributes(folder.toPath(), "unix:dev,ino", LinkOption.NOFOLLOW_LINKS)
            "${a.getValue("dev")}:${a.getValue("ino")}"
        } catch (_: UnsupportedOperationException) { Os.lstat(folder.path).let { "${it.st_dev}:${it.st_ino}" } }
          catch (_: IllegalArgumentException) { Os.lstat(folder.path).let { "${it.st_dev}:${it.st_ino}" } }
    }
    internal fun invalidate() {
        val pair = synchronized(lock) { prepared to operations.toList() }
        pair.first?.close(); pair.second.forEach { it.cancel() }
    }
    override fun close() {
        synchronized(lock) { if (closed) return; closed = true }
        invalidate(); expiry.shutdownNow(); worker.shutdown(); control.shutdown()
        // Cancellation remains admitted until active operations have retired.
    }
    internal fun awaitClosed() {
        awaitStopped(worker, control, expiry)
        cancellations.shutdown(); awaitStopped(cancellations)
    }
}
