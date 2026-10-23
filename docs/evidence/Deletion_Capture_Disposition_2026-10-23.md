# Combined document deletion and pending-capture disposition — 23 October 2026

Source checkpoint: `e8edc37`, branch `phase-a/backend`.
Backend acceptance remains NOT MET and production H.I. remains gated.

Full validation passed against matching clean source manifests: **224 Rust tests**,
**118 Android tests**, ten desktop JNI smoke groups, format, strict Clippy, release,
traceability, ARM64/ELF, Android lint, AAR payload and manifest checks. No tests failed,
were ignored or skipped in this run. Commands, hashes, timings and logs are archived
in `continuation-2026-10-23/deletion-capture-review/` and `latest/`.

## Delivered behavior

Native deletion preparation identifies currently authorised, non-purged Note/Canvas
capture owners in the reviewed subtree. ValidateDeletionReview checks a held token's
current generation and full subtree content authority without consuming it. Capture
owner metadata is never a new content grant; protected roots/children still require
current authority, and purged descendants are excluded. Confirmation retains its
separate final checks and token consumption. Existing review capacities/expiry apply.

Typed Android prepareDeletionWithCaptures combines this native review with exact
checksum-bound records from the active journal and recognised legacy Entity Capture
outbox. The public preview contains dependency metadata and draft tickets/owners/sealed
flags, not raw ink. Recovery authority is checked for matched records, including the
existing target-specific proof for retired contexts. Unattributable legacy/incomplete
evidence is counted without owner/content disclosure and retained. Unrelated drafts
are excluded from the selected subtree manifest.

Confirmation is one admitted writer action: validate current native review authority,
resnapshot/hash records, recheck their recovery access, compare the exact reviewed set,
then submit native deletion for a real acknowledgement. Changed prefixes/record sets,
relock, expired/stale reviews or failed/ambiguous native acknowledgement remove no
recovery copies. A true attempt consumes/releases its review; false confirmation
rejects before admission. Callers can release an unused review explicitly.

KeepDrafts preserves recovery copies. DiscardReviewedSealed disposes only those exact
reviewed sealed copies after successful native acknowledgement, as part of the same
authorised operation. Active, unknown, unrelated and later-arriving work is retained,
without synthesising completion or retargeting. This does not grant subsequent raw
read access to a now-retired context. A caller may still deliberately resubmit its
own original observations through normal capture admission; deleting a recovery copy
is not document Undo or a blanket revocation of caller-owned data.

The receipt separates the committed document action from draftsRemoved, keptActive,
keptSealed and cleanupUnconfirmed. Post-commit cleanup failure sets attention and does
not falsely acknowledge file removal or implicitly roll back the document. An
unconfirmed filesystem result can be uncertain, not necessarily a known retained file.
Existing scoped recovery review/discard can resolve remaining evidence. Cancellation
of a future cancels observation only. SQLite and private files are separate durability
boundaries; no atomic cross-store deletion is claimed. Legacy raw JSON deletion stays
document-only and keeps recovery by default. No schema, journal or source format changes.

## Executed coverage and limits

Two new Rust fixtures verify target-manifest privacy, omission of purged descendants,
non-consuming token validation and current generation/proof checks. Four Android
actual-JNI fixtures cover mixed-store subtree disposal with active/unrelated/unknown
retention, exact-byte keep plus relock rejection and permanent purge, active-prefix
change rejection followed by normal source commit, and injected pre-removal failure
after native Trash acknowledgement followed by scoped recovery/discard. The mixed-store
fixture injects pending legacy records while the app writer is idle to simulate
unacknowledged records; this is synthetic fault setup, not a production secondary
writer. Fault injection only fails cleanup; it cannot grant access or acknowledge it.

Existing record/byte/review bounds apply. Snapshot parsing/hashing and subtree review
have no constant-RSS or latency guarantee. Complete legacy/incomplete content review,
unsupported/malformed recovery paths, authoring/pane configuration, operation history,
resource scheduling, rendering and portability remain open. Prior E01 (initial-Create
fixture acknowledgement timeout) did not recur in this run; its cause remains
unestablished and is now explicitly retained in the A7 gap text. Older failed/isolated
attempts remain in the capture-privacy evidence archive; this pass does not erase them.

All eight gap groups remain OPEN. The gate exits 2 solely for those groups and
canonical/inherited coverage, without stale automated-evidence reasons. The ledger
retains 94 requirements, 103 inherited clauses and 158 candidates without inferred PASS.
The handover folder is ignored, untouched and absent from tracked files/reachable path
history. Local commits only; nothing pushed. No production H.I., model, recogniser,
external inference or automatic OS backup change.
