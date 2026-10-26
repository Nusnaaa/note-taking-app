# Native subset v13 — source-unit table layout

Writers emit v13; readers accept known versions 1–13. Earlier versions retain their field defaults. A table may now include `layout` with `coordinate_space`, `origin`, `row_heights`, `column_widths`, and optional `ink_origins`. Each origin identifies a table-local `cell`, globally identified original `stroke`, and its source-unit cell origin at capture or explicit adoption.

Layout dimensions must match the logical grid; sizes and resulting bounds must be finite and positive. Stroke origins must be unique and cover exactly the table's cell-region memberships. Visible table strokes require a mapping. Source records must belong to the block and use the declared coordinate space. Raw observed points remain immutable. Current placement translates original source vectors by current cell origin minus capture cell origin, then applies the block transform. Resizing does not scale ink.

Geometry under a header older than v13 is unsupported, including geometry retained only in block history. Independent copies remap cells and source-stroke references in current/history geometry. SQLite schema21 and both encrypted library backup formats use native-v13 semantics, including before/after Undo payload feature checks. Existing schema20/v12 cell aliases and receipt bytes are not rewritten by migration.

This format extension does not establish complete scene fidelity, device calibration or completed backend acceptance.
