# Backup indexed seeks and capability cache — 24 October 2026

Source checkpoint: `a6fbf2a`, branch `phase-a/backend`.
Backend acceptance remains NOT MET; production H.I. remains gated.

## Delivered behavior

Backup row lookup uses separate first-row, exact-rowid and next-rowid statements.
Nullable OR predicates no longer force prefix scans. Live-cache class filtering no
longer scans a run of omitted rows inside one cursor step: rows advance individually,
with CASE returning NULL for the blob and its following digest column. Retained rows
preserve their original field order, bytes, digest and export-authority validation.
Unselected asset tables still skip as a whole. No additional SQLite connection opens
and no read transaction persists between pulls; native exclusive ownership is intact.

Access now carries a private opaque allocation identity. Authentication and relock
replace that identity. A backup cursor caches only a successful required-scope check
for the exact unchanged identity, retaining an Arc to prevent allocator address reuse.
It still checks current/private-proof clock expiry, app protection, source generation
and cancellation on every pull. New/replaced identities require full scope validation;
the first validation polls cooperative control between groups of checks. The identity
is neither serialisable nor a permission grant, and never crosses JNI/JSON/model input.
Private scopes remain immutable outside the existing authenticated/relock operations.
Failed cursors remain poisoned and cannot revive after reauthentication.

Original ink, logical/encryption/SQLite/receipt formats, model readiness/consent, source
retention and explicit no-overwrite publication rules remain unchanged. No SDK/model,
network permission, production H.I. or numeric device SLO is introduced.

## Executed coverage

A query fixture checks SQLite plans for indexed exact/next seeks, includes negative
first-row IDs, checks live-cache NULL projections and retained-byte column ordering,
and compares VM work with the previous filtered/nullable query over a long cache run.
That is relative algorithm evidence in a fixed fixture, not a tablet performance claim.

A security unit fixture expires the same capability after a successful cached pull,
without rotating its identity. Integration replaces access at the same Rust variable/
address with unrelated authenticated scopes, then relocks a cached grant; both deny
and permanently poison the old cursor. Another complete pull/encrypt/restore fixture
over 130 live caches plus an authorised historical snapshot verifies cooperative yields,
absence of private cache marker bytes and exact retained snapshot fidelity.

Development passed the library-unit group, five backup-pull fixtures, eight streaming
backup fixtures including a whole library above 64 MiB, capture-privacy and host suites,
and strict workspace Clippy. Final frozen-source validation passed at a6fbf2a: **283 Rust tests**, **135 Android
tests**, ten desktop JNI smoke groups, format, strict Clippy, release, traceability,
ARM64/ELF, Android lint, AAR and merged-manifest policy. Both runners finished with
clean matching source manifests. No tests failed, were skipped or ignored in these
runs. Commands, hashes, timings and raw output are archived under backup-seek/ and
latest/. Android results establish boundary/build regressions, not a JNI backup
workflow. E01 did not recur; its cause remains unestablished.

## Remaining work

Initial metadata preparation and first scope validation still scale with the library;
fully scheduled/bounded preparation remains open. Native Host job admission/registry,
host/capture epoch cancellation and actual worker/sink retirement through close,
JNI/Android backup transport and platform sink durability/publication remain open.
Restore semantic validation still materialises a whole graph; disk quota/crash-orphan
cleanup/review and comprehensive interruption/resource/device evidence remain open.
Other backend gaps and whole-requirement semantic coverage remain incomplete. E01
retains its separate historical follow-up.

The handover folder remains ignored, untracked and absent from local path history;
commit/push guards remain intact. No push was performed.
