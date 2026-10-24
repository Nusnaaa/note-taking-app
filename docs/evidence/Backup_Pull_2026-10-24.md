# Bounded authoritative backup pulls — 24 October 2026

Source checkpoint: `71ef854`, branch `phase-a/backend`.
Backend acceptance remains NOT MET; production H.I. remains gated.

## Delivered behavior

SQLite-managed exclusive native ownership intentionally prevents a second read
connection from opening the live library. It is preserved. StreamBackupCursor provides
bounded incremental pulls through the authoritative connection: at most 64 KiB and
64 cursor steps per call, with no retained transaction or SQL statement on return.
Encryption, passwords, caller sinks and external callbacks are absent from the pull.
Unselected source-asset tables/live-cache rows avoid reading blob payloads. The cursor
keeps logical position and bounded single-cell engine-metadata scratch, not a library
snapshot. Engineering limits are not device SLOs or whole-process memory measurements.

Every pull checks the bound library/generation, an exact-expiry fresh backup proof,
current supplied Access and JobControl. Source generation is checked again outside the
read snapshot. Source writes between pulls invalidate the cursor before another chunk
can be released. Denied/cancelled/stale or prematurely completed cursors are poisoned;
reauthentication cannot revive them. Logical EOF and validated completion are separate
from an empty yielded chunk. Callers validate before authenticated encrypted completion
and recheck after sink completion before acknowledging success.

The existing finish_stream_backup convenience path now uses the same cursor and checks
source/authority around finalisation. It is still a synchronous convenience; callers
must use the pull/worker split to interleave authoring. Wire envelope/body/schema,
original stroke payloads/hashes, deliberate protected whole-backup consent, independent
source retention and legacy restore behavior remain unchanged. This is a move-only,
non-deserialisable trusted core capability, never ordinary JSON or model input.

## Executed coverage

Three new integration fixtures cover bounded pulls spanning large Unicode cells and
exact restored original ink, current-scope denial and permanent cancellation, stale
generation, incomplete finalisation, and an actual blocked ciphertext sink on a separate
worker while original ink commits on the Store owner. The stale export then fails and
its partial encrypted stream lacks authenticated completion. A new exclusive-owner unit
fixture verifies authoring can commit after a pull while second SQLite connections stay
denied, including reopen integrity after the owner retires.

Development passed those fixtures, the full library-unit group, all eight streaming
backup fixtures (including the real library above the legacy 64 MiB cap) and strict
workspace Clippy. Final frozen-source validation passed at 71ef854: **279 Rust tests**, **135 Android
tests**, ten desktop JNI smoke groups, format, strict Clippy, release, traceability,
ARM64/ELF, Android lint, AAR and merged-manifest policy checks. Both runners finished
with clean matching source manifests. No tests failed, were skipped or ignored in
these runs. Commands, hashes, timings and raw output are archived under backup-pull/
and latest/. Android evidence is regression coverage, not an implemented JNI backup
workflow. E01 did not recur; its cause remains unestablished.

## Remaining work

Native Host job registry/admission, host/capture epoch cancellation, worker/sink actual
retirement through close and JNI/Android backup transport remain open. Whole-library
review preparation still scans metadata. Cursor-step/byte admission is not a bound on
SQLite work inside a query or on per-pull permission-set traversal: optional-parameter
rowid predicates, filtered asset-row scans and repeated large scope checks need query/
capability optimisation and scale validation before host integration. Fully scheduled/
bounded preparation, platform
sink durability/publication, globally bounded semantic restore validation, staging disk
quota and crash-orphan cleanup/review also remain open. No new SDK/model, network
permission or production H.I. is introduced. Other backend gap groups and semantic
coverage remain incomplete. E01 retains its separate historical follow-up.

The handover folder remains ignored/untracked and absent from local path history, with
commit/push guards intact. No push was performed.
