# ADR 008 — scheduled-backup authority foundations

25 October 2026. In-progress backend implementation, not enabled scheduling.

Use separate authority for persistent destination access, device credential
storage and running an automatic backup. Choosing a manual backup destination or
possessing a historical publication record must not silently enable any of them.
Keep credentials out of the publication catalogue and the portable notebook.

Use platform Keystore AEAD wrapping with purpose/library/credential binding and
explicit resource retirement. Do not export the wrapping key or silently fall
back to a plaintext/software-stored credential. Preserve portable passphrase and
offline recovery-key independence when a device key becomes unavailable. Record
durable store intents before creating keys and retain possible committed state
for explicit review after uncertainty. Internal credential persistence now follows the [policy-store contract](../formats/BACKUP_POLICY_STORE.md); production controller integration is pending.

For a provider, record deliberate persistent-access consent before requesting OS
persistence. Preserve pre-existing read/write facts and return possible effects
after the irreversible call. Never revoke a shared UID permission as guessed
rollback. Capability epochs must cover in-flight inspection and verification.

Future jobs must use bounded workers and the same capture/recovery/source guards,
with app/library ownership held until actual work retires. The OS may stop a job;
its callback is not proof that provider or key operations have retired. See
[JobService](https://developer.android.com/reference/android/app/job/JobService).
Protected-content authority must remain explicit; no generic approving verifier
or ordinary access grant may silently bypass locks. Persisted configuration/run state now uses revision-bound Arming/Enabled/Disabled
transitions and atomic referencing-policy disablement during credential retirement.
Shared runtime acquisition is now implemented under [ADR 009](ADR-009-shared-notebook-runtime.md). The public scheduling/credential/consent controller and primary-root service are
now integrated under [ADR 010](ADR-010-opt-in-scheduled-backup-workflow.md).
Protected-content delegation and independent-root dispatch remain unfinished.

These foundations do not change Rust portable-core ownership, encryption of
portable backups, no-cloud defaults or backend-before-H.I. sequencing. See the
[component contract](../formats/SCHEDULED_BACKUP_FOUNDATIONS.md).
