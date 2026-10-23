# Revision-bound selection bookmarks — 23 October 2026

Source checkpoint: `04d5e25`, implementation `356f2bd`, branch `phase-a/backend`.
Backend acceptance remains NOT MET and production H.I. remains gated.

Full exact-source validation passed with matching clean source manifests:
**180 Rust tests**, **101 Android unit/Robolectric tests**, and **ten desktop JNI
smoke groups**, with no failed/ignored/skipped tests. Format, strict workspace
Clippy, release build, traceability, ARM64 compilation/ELF, Android lint, AAR native
payload and isolated-service manifest checks passed. Commands, source hashes and
logs are retained in `continuation-2026-10-23/selection-bookmarks/` and `latest/`.

## Delivered behavior

Saved views optionally retain directional anchor/focus selection bookmarks. Equal
endpoints represent a caret; cross-block selections retain endpoint order. Each
point names the exact block revision, optional recognition region and a typed target.
Text/numeric/code and label offsets count Unicode scalars. Maths points use local
presentation-node identity and scalar leaf/ordered-child boundaries. Targets include
table-cell text/maths, diagram labels, graph axis/unit/series labels, image captions,
whole-content boundaries and actual visible source samples. No content text is
copied into a bookmark, normalized, solved or repaired; observations stay immutable.

SaveView validates current visible targets, exact revisions, owner/exercise scope,
current block/exercise protection and optimistic view revision transactionally.
RestoreView returns settings and an explicit None/Current/NeedsReview state. Changes
never silently clamp a stale offset or rebind it to replacement content. Stored View
metadata is not itself permission to apply a bookmark. Protection changes gate all
metadata and replacement/clearing using current authority. Exercise reassignment
leaves an out-of-scope bookmark review-required; independent master/exercise views
retain independent positions. Future revisions, wrong component types, unknown
current targets and invented/foreign source samples fail closed. Source sample
references additionally verify source ownership/digest and persisted-stroke validity.

Schema 11 versions optional saved-view bookmarks without rewriting legacy payloads.
Native subset v6 preserves/remaps global block/region/source references and reads
v1-v5; node/cell/part/series IDs stay local to copied content. Stale positions remain
stale after copy; retired region references receive only remapped bookmark IDs, not
new source content. A native tag below v6 cannot hide bookmark fields. Encrypted
backup schema 11 preserves current/stale metadata and restores schemas 8/9/10 with
corresponding legacy semantics; older schema tags reject the extension. Selection-
only native exports keep bookmark endpoints inside the selected block boundary.
Original ink/capture receipt fingerprints remain independent of these settings.
The host queue now boxes Request envelopes to avoid inflating every message by the
larger optional view metadata.

## Executed scope and limitations

Seven new position fixtures cover Unicode/direction, reopen, stale/future/out-of-range
positions, rollback, maths child/leaf and component targets, cross-block ranges,
foreign owners, fresh protection, reassignment, original samples, native ID remap,
stale copy/encrypted backup, downgraded tags and byte-preserving schema-10 migration.
A host fixture and actual desktop-JNI/Robolectric fixture save and restore a Unicode
position, explicitly edit the source and require review while retaining the original
bookmark revision. Existing capture, receipts, migration, portability, backup,
privacy and Android suites remain included.

During targeted development, a fixture attempted a whole-block replacement of
partitioned ink and correctly received ReviewRequired in `cargo test -p stem-core
--test positions --test interrupted_capture --test objects --test recovery
--test exercise_migrations --offline` (targeted run exit 101). It was corrected to use the
explicit region-edit API; no protection bypass was added. Strict Clippy findings
were corrected (boxed message envelope, range syntax and export placement). One old
exercise fixture's current-format assertion was updated from v5 to v6. The final
complete run validates the corrected committed source without skipped tests.

This increment does not implement caret rendering, gestures, automatic text/math
rebasing or production H.I. Full workspace/last-document/pane restoration and
all-operation history/retirement remain open, along with rich-object flow/export,
model lifecycle, scalable portability and other gap-register work. Unicode scalar
positions do not claim grapheme-level tablet editing semantics. Device performance,
pen quality and physical process/storage interruption acceptance are not established.
No models/weights, handwriting upload, remote inference, automatic OS content backup
or origin push is introduced. The handover folder remains ignored and untracked,
with local commit/push guards installed.

The backend gate exits 2 only for substantive open implementation groups and
incomplete canonical/inherited coverage; no stale or incomplete automated evidence
reason remains.
