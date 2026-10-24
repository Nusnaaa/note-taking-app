# Incremental encrypted backup and private restore — 24 October 2026

Source checkpoint: `aff1137`, branch `phase-a/backend`.
Backend acceptance remains NOT MET; production H.I. remains gated.

## Delivered behavior

New trusted core APIs prepare read-only library/generation-bound review metadata,
then export a consistent SQLite snapshot as fixed-table typed records into bounded
STEMENC2 encrypted frames. No whole-library Snapshot/serialized plaintext Vec is built
by this export path. Fresh original-ink consent/protected scope, independent source
retention/export authority, model-resource exclusion and stale-plan rules apply.
The generation is checked outside the read snapshot before authenticated completion.

RustCrypto aead 0.5.2 STREAM BE32, already resolved transitively in Cargo.lock, is now
an explicit dependency with its stream/std features. No dependency version was upgraded.
It wraps XChaCha20Poly1305 with per-stream random data/nonce material, independent
password/recovery-key wrapping and the unchanged Argon2id/password rules. Fixed bounded
frames authenticate their header/length/final flag. Reader budgets, ordered STREAM
state, mandatory finality and exact underlying EOF reject partial/reordered/spliced/
appended data. Poisoned sink state cannot be retried into a false recovery receipt.
A successful generic sink flush is not a claim of file sync or OS publication.

Restore inserts records into a random create-new owner-only SQLite file in a trusted
private directory, never a live library. Authenticated finality/EOF precede its private
transaction commit. The existing complete logical/integrity validator then runs before
any Store is returned. StagedBackupRestore owns connection/scratch retirement. Explicit
fresh-authenticated no-overwrite publication reuses the existing synced restore path.
Original ink/history/protection remain intact; source grants/readiness are not portable,
source revocation remains independent, and library identity is regenerated. The private
staged working DB is plaintext under the existing working-database privacy boundary;
public backup output is encrypted only. Android transport must allocate scratch under
no-backup storage. Ordinary drop cleanup is not crash cleanup.

Legacy STEMENC1 and logical schemas 8–15 remain readable through the existing API;
its in-memory bound is unchanged. SQLite/native/receipt schema versions do not change.
STEMLIB2 is a new fixed-table transport of current logical schema 15, not arbitrary SQL.
The new individual-cell and existing million-row admission caps are engineering bounds,
not approved device SLOs. See HOST_INTEGRATION.md for caller budgets, private-directory
permissions, completion and publication obligations.

## Executed coverage

Eight new backup fixtures cover actual above-limit whole-library export/restore,
original ink/history/Undo, independent new identity and reopened no-overwrite publication,
explicit retained-source inclusion/revocation, protected fresh proof, source generation,
malformed/truncated/appended data, quotas, cancellation and private-directory rejection/
cleanup. The large library is constructed through ordinary authoring APIs; legacy
prepare_backup returns ResourceLimit while the stream path roundtrips it and its ink.

Six encryption fixtures cover password/recovery, arbitrary writes/flushes, independent
RustCrypto primitive decoding, malformed length/finality, ordering/duplication/splicing,
truncation/trailing data, budgets and permanent sink-failure poisoning. A separate
above-64-MiB generated stream roundtrips through bounded I/O with a checked digest.
No generated archive, key, model, credentials or private handwriting is committed.

An initial restore fixture batch failed because tempfile's default directories had
shared permissions. Tests were corrected to request owner-only directories; production
permission checks and assertions were kept. The failing log is preserved under
streaming-backup/development/default-directory-permissions.log. All eight reran and
passed; the pre-fix error-only cases were not counted as integrity coverage. The initial
Android invocation also stopped at argument validation, before any app check; its
reported failure/uncertain cause is retained separately.

Development passed the eight backup fixtures, six encryption fixtures, nine existing
legacy backup fixtures and strict Clippy. Full frozen-source Rust validation passed
**275 tests**, format, strict Clippy, release and traceability. Final Android regression validation passed **135 tests**, ARM64/ELF, ten desktop JNI
smoke groups, lint, AAR payload and merged-manifest checks. Both runners finished
with clean matching source manifests at aff1137. No tests failed, were skipped or ignored
in the final runs. Commands, hashes, timings and raw output are archived in the
streaming-backup root and latest/. Android checks establish boundary/build regressions,
not Android streaming-backup transport, which remains unimplemented. E01 did not recur;
its cause remains unestablished.

## Remaining work

Host/JNI/Android backup transport, source/permission cancellation and capture-priority
scheduling, platform sink sync/publication, staging disk quota and crash-orphan cleanup/
review remain open. The common semantic validator still materialises a whole logical
graph; this increment does not claim globally bounded restore memory. Comprehensive
interruption/resource/device coverage and other backend gap groups remain open. E01
retains its separate historical follow-up. No new SDK/model, network permission or
production H.I. is introduced.

The handover folder remains ignored, untracked and absent from local path history;
local commit/push guards remain intact. No push was performed.
