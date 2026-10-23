# Reviewed pending capture retargeting — 23 October 2026

Source checkpoint: `303d1db` (implementation `1055a11`, then refreshed traceability maps), branch `phase-a/backend`.
Backend acceptance remains NOT MET and production H.I. remains gated.

## Delivered behavior

The Android backend can explicitly review a sealed active-journal draft for a different
Base/Custom layer within its active canvas. The review binds the original recovery
checksum and selected destination to the current layer revision. Confirmation freshly
checks current source-context authority, active destination state, exact recovery bytes,
absence of a committed native receipt, absence of any conflicting legacy envelope
ticket, current full-canvas permissions and unchanged layer revision/destination.
Relocking, stale evidence, missing/changed layers and retired contexts reject before
private routing changes. Note and cross-entity destinations are not accepted.

Only the pending route and recovery revision change. Source ID, capture session, begin
order, original points/channels, style, tool and observed ending remain identical.
Ordinary checkpoints cannot rebind destinations. The selected private route is published
before recovery approval and normal per-session FIFO replay. Earlier blocked work can
still defer document admission. The typed receipt distinguishes private checkpointing
from actual SQLite commitment; file and database durability boundaries remain separate.
An error after publication is not a promise of rollback or a document-save receipt.

Committed and ambiguously acknowledged captures cannot be retargeted. Retrying an old
caller-held session frame now probes SQLite before recreating a missing journal record,
even if the session still remembers its ticket. This avoids leaving stale routing
records after recovery/replay retired the file. Native target/source receipt matching
still rejects an old destination without duplicating observations.

## Executed coverage

Six new Android fixtures cover sealed/active/stale/implicit rebinding rules, unchanged
source on retarget and reopen, injected publication failures before and after rename,
actual JNI Base/custom membership, stale layer revision, false confirmation, fresh
relock/authentication, old-frame retry, native commit with lost acknowledgement and
conflicting unclassified legacy envelopes. Existing retired-target review fixtures now
also reject retargeting. Failed private publication retains a readable original or
selected route with exact observations and the existing recovery-review requirement;
it does not claim simulated filesystem power-loss durability.

Development passed all 129 tests and lint. A preceding run passed 127 tests but failed
lint on a nested cancellation branch's indentation; explicit braces fixed it without
suppressing lint or weakening assertions. The original lint text is retained as
continuation-2026-10-23/capture-retarget/development-lint-failure.txt. These were working
tree development runs; their results are not labelled clean-source acceptance.
The first frozen-source run at 1055a11 passed format, strict lint, all 230 Rust
tests, release and the complete 129-test Android validation, but failed traceability:
the final fixtures had been added after generating the line references. All original
artifacts are retained under capture-retarget/attempt-1/. Maps were regenerated and
checked, then committed as 303d1db; no implementation/assertion was changed to bypass
that failure. Complete corrected-source validation passed on matching clean manifests at 303d1db:
**230 Rust tests**, **129 Android tests**, ten desktop JNI smoke groups, format,
strict Clippy, release, traceability, ARM64/ELF, Android lint, AAR payload and manifest
checks. No tests failed, were skipped or ignored. Raw commands, hashes, timings and
output are archived in capture-retarget/ and latest/. E01 did not recur in this run;
that observation does not establish its cause or close its follow-up.

## Boundaries

Cross-entity and full legacy/incomplete-record retargeting, complete scene rendering,
optional-work scheduling and the other gap-register groups remain open. No production
H.I., handwriting egress, recogniser/model installation or network permission was added.
Prior E01 PDF fixture setup-timeout evidence remains open; its cause is unestablished
and no timeout/assertion was weakened or test disabled here.

The handover directory remains ignored, untracked and absent from reachable Git path
history. Positive staging paths exclude it; it was neither committed nor pushed.
