# Native subset v4 — selected stroke regions

22 October 2026. This extends v3 without changing the preserved v1–v3 source documents.
It is a portable authoring subset, not a claim of complete application export.

- Each block has `regions`, defaulting to an empty array when reading old data.
- A region has its own stable ID, a non-empty, non-overlapping selection of that
  block's original stroke IDs, and optional editable prose/numeric/code/maths content.
- Region IDs may recur in retained revisions of the same block. Cross-kind identity
  collisions and sharing one region identity across separate blocks are rejected.
- A region proposal binds to the original block revision, region identity and exact
  selected-source fingerprint. Explicit acceptance changes only its chosen region.
- Split/merge operates on unconverted regions. It refuses to discard already accepted
  editable content. Source samples and acquisition order remain in their original block.
- Independent import remaps current/historical region and stroke identities consistently.
- v1–v3 reads supply empty regions. Writing regions under an older format version is
  rejected. Schema-9 whole-library backups retain region content and undo records.

The native subset still refuses PDF-linked documents. Complete source-policy portability,
import lineage and the remaining scene features are not claimed by this version.
