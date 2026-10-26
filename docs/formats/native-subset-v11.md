# Native subset v11 — table-cell ink targets

Version 11 extends v10 with explicit handwriting regions inside structured tables.
A table remains `Content::Table`; each cell retains its stable `Cell.id` and typed
`CellValue` (text, numeric string or mathematical presentation). A table block's
`regions` use the selected cell ID as the region ID. Each region owns a disjoint
set of that block's observed stroke IDs. Optional region content records the
accepted representation and must exactly match the associated cell value.
Unknown cells, overlapping source assignments and mismatched representations are
invalid. Whole-table recognition is refused; the existing region snapshot route
selects only the chosen cell's currently visible source strokes.

`CaptureCell` binds ticket, table block, expected block revision, cell and exact
ordered strokes. A matching receipt acknowledges the existing capture; changing
any binding is a conflict. Captures use the existing persisted-stroke contract,
1000-stroke input-batch limit and a 4096-observation bound per table block. These
are engineering admission limits, not performance SLOs. Caller coordinates remain
observations in table source space. The operation does not infer a grid, clip
points, recognise handwriting or calculate physical cell dimensions.

Accepting a cell proposal atomically updates that cell's value and its region
representation; neighbours remain unchanged. Corrections and new captures advance
the block revision, invalidating old proposals. New ink clears the region's
accepted marker while retaining the prior editable cell value until review. Raw
observations remain in the owning block independently of acceptance, undo and
history pruning. Undo can hide a capture but cannot delete its observations; redo
compares the historical presentation while retaining an append-only source suffix.

Independent native copies remap cell, region and stroke identities together,
including current and retained historical table states. Derivative exports emit
accepted cell values once, warn about unconverted cell ink and include raw ink
only through the existing explicit original-ink choice.

Readers support versions 1–11. Any table-region binding in current/history content
under a pre-v11 header is unsupported. SQLite schema 19 is a semantic compatibility
fence: no table columns are added, but older application versions must refuse the
library rather than misinterpret a cell target. Whole-library JSON writes 19 and
reads 9–19; streaming writes 19 and reads 15–19. Physical table order is unchanged
from schema 18. Schemas below 19 validate against the older native semantics and
must not smuggle table-cell regions through current content, retained history or
Undo before/after rows. Original stroke coordinates and lexemes are unchanged.
