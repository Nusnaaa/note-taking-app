# Persisted scheduled-backup policy

`BackupSchedulePolicy` shares the purpose-specific device-private policy SQLite
file with credential intents and provider consent. It registers no OS job itself and is
used by the [reviewed controller](SCHEDULED_BACKUPS.md) owned by NotebookBackend.
It never enables anything from a manual backup/history row alone.

The current configuration is library-bound, with a stable schedule UUID and a
fresh revision UUID for each reviewed replacement or disablement. It records the
chosen directory/tree, credential reference, provider-consent decision where
required, requested interval, original-ink/source-asset choices and explicit
scheduling consent time. Version 2 stores an explicit positive encrypted-backup
byte limit and reviewed destination identity against the same revision; old known
v1 configurations migrate to Disabled instead of guessing those choices. The future controller must obtain fresh verification,
review the actual destination, validate the password through native initial
backup creation and query platform interval constraints. The repository's
positive interval check is defensive storage validation, not a platform timing
promise or invented responsiveness target.

A new/replaced configuration is Arming. It becomes Enabled only when the caller
acknowledges registration of the exact schedule and revision. A lost registration
acknowledgement remains discoverable; no automatic startup repair guesses that a
job is registered. Disabled changes the revision before OS cancellation, rejecting
old job callbacks even if cancellation is delayed or uncertain. Replacing a policy
requires its exact current revision. No policy is implicitly enabled after
credential failure or restart.

Arming and job admission check a Ready SchedulePassword for the same library in
the same SQLite transaction. A provider policy additionally requires the exact
tree's original consent decision and known Persisted/AlreadyHeld outcome with
both observed rights. An Uncertain outcome does not authorize scheduling even
when a rights observation was available. Runtime still needs fresh provider and
app/source/capture checks; past permissions and registration are not availability
or protected-content authority guarantees.

Run admission atomically records a fresh run UUID and claims the configuration's
single active-run fence. Duplicate/concurrent/stale/disabled callbacks cannot
admit another run. Disablement retains this fence until actual work retires, so a
replacement cannot race a surviving worker. Finishing records one immutable
outcome and releases only that run's fence; it cannot re-enable a disabled or
reconfigured schedule. A known publication may refer to the independently
acknowledged publication catalogue sequence. Uncertain/failure outcomes cannot
carry a successful publication reference. No secret or notebook payload is copied
into run records.

Credential retirement disables and changes the revision of any referencing
configuration in the same transaction as Ready/Intent -> Retired. Rollback affects
both changes; lost acknowledgement preserves both. Key/consumer retirement then
follows the vault protocol. A running job's fence stays intact until retirement.
A replacement needs a newly usable credential and fresh reviewed scheduling
consent, not an old automatic restart.

After acquiring exclusive library ownership and proving all previous workers
are gone, the future runtime can reconcile an unfinished run as Interrupted.
Calling this on job stop or cancelling a future alone is not sufficient. This
state never claims that an external publication had no effect; independent
catalogue/provider reconciliation and user-visible uncertainty remain required.
Run discovery is bounded library-scoped keyset pagination. No device performance,
OS wake-up timing, durable remote storage, background protected-content access or
physical process-loss acceptance is established by this repository.

The public consent/credential controller, recovery-key management, shared-runtime
acquisition and primary-root JobService are now implemented. Still required:
protected-backup delegation and independent-root dispatch, with actual OS/device/
provider/Keystore/interruption acceptance. The policy repository does not itself
make a complete-backend acceptance claim; no production H.I. is added.
