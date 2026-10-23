# Retired capture recovery and subtree purge — 23 October 2026

Source checkpoint: `88b9acb`, branch `phase-a/backend`.
Backend acceptance remains NOT MET and production H.I. remains gated.

Full exact-source validation passed: **207 Rust tests**, **109 Android tests** and ten
desktop JNI smoke groups, with no failed/ignored/skipped tests. Format, strict Clippy,
release, traceability, ARM64/ELF, Android lint, AAR payload and manifest checks passed.
Both validators recorded matching source SHA-256 manifests and the same clean source
commit. Commands, times, hashes and logs are archived in
`continuation-2026-10-23/retired-capture-recovery/` and `latest/`.

## Delivered behavior

`CaptureRecoveryAccess` checks authority to review private uncommitted capture evidence
without granting document access. Its only data is Active/Trashed/Purged state. Current
app and entity/ancestor locks apply, including retained purged ancestor tombstones.
Any retired context additionally requires a currently valid target-specific Entity
proof, even when formerly public. Unknown/misbound identities and containment cycles
fail closed. Target-kind validation follows permission checks. Ordinary document read,
restore and capture APIs retain their existing rejection of purged destinations.

Android recovery discovery/review now includes all sealed pending active-journal
records, including completed/lifecycle strokes blocked by save failure. Active live
contacts stay excluded. The review includes target state and the exact observed
ending/style/space/samples. Explicit discard requires confirmation, fresh recovery
authority and the reviewed record checksum; it removes only that private journal copy.
It does not undo an ambiguous document commit, discard unrelated records or ban later
explicit resubmission by the original caller. Sealed records do not need an invented
ending or unnecessary metadata rewrite to retry. The existing binary format remains
unchanged. Recovery remains bounded private no-backup storage, not external export.

Acceptance still checks ordinary current document access before journal approval and
native replay. It cannot restore, retarget or replay into a retired destination. An
explicit Trash restore may permit later acceptance of the unchanged evidence; purge
leaves scoped review/discard available. Relock is enforced again at discard/accept.

The new real subtree-purge fixture exposed a pre-existing ordering bug: mutating the
parent tombstone before checking children caused their inherited-authority checks to
fail and roll back an otherwise authorised purge. Purge now preflights all descendant
access/content protection before mutations in the same SQLite transaction. Denial or
injected commit failure preserves the original ink; authorised whole-subtree purge
retires it coherently. Tombstone identities/locks/parent links remain for honest broken
references and recovery authority.

## Executed coverage and limits

Three Rust fixtures execute real Trash/restore/purge and reopen, target/ancestor/app
proof combinations, blocked ordinary reads/capture, non-surface privacy checks,
protected-child denial, failure rollback and exact source retention before authorised
permanent deletion. An Android journal fixture covers sealed completed-record review,
checksum conflicts and active-contact exclusion. An actual JNI/Robolectric fixture
uses deliberately synthetic persisted Trash/purged states, verifies authentication,
reviewed state/sample retention, replay rejection, relock and discard. It does not
claim execution of the still-unexposed host deletion workflow. All prior tests reran
against the final rebuilt library with the full validator's forced Gradle rerun.

Host-facing Trash/restore/permanent-deletion review workflows remain open and are now
explicit in A1. They need backend-held, generation-bound review capabilities and fresh
confirmation checks, not caller-forgeable serialized review objects. Legacy outbox,
corrupt/incomplete-record recovery and complete pending-draft disposition during deletion
remain open. No full recovery, device interruption or latency guarantee is inferred.

All eight gap groups remain OPEN. The gate exits 2 solely for those groups and
canonical/inherited coverage, without stale automated-evidence reasons. The ledger
retains 94 requirements, 103 inherited clauses and 158 candidates; no PASS is inferred.
The handover folder is ignored, untouched and absent from tracked files/reachable path
history. Local commits only; nothing pushed. No production H.I., recogniser, model,
external inference or automatic OS backup change.
