# ADR 042 — explicit typed-table structural edits

Status: partial Q071 implementation. Ink-aware cell layout, resizing and cell move/copy remain open.

`TableEdit` inserts/removes one row or column at an explicit index. Existing cells keep their local IDs and authored values. Inserted cells receive fresh IDs and empty text, without numeric inference. Inserting strictly inside the header extends it; insertion at the header/body boundary creates a body row. Removing a header row reduces the header count. Removing the final row or column is invalid; deleting a table uses the existing block deletion workflow. Bounds, rectangular shape, ID uniqueness and existing table allocation limits are checked on a detached candidate before replacement.

`Store::edit_table` and host `EditTable` use the existing fresh-authority/revision-checked transaction, retained block history and compound source Undo machinery. Invalid edits leave the stored block unchanged. The operation changes only existing serialized table fields, so it introduces no new persisted feature or schema version.

Structural edits currently return `ReviewRequired` when a table has retained source ink or regions. Cell placement is not yet modeled; moving logical cells alone would leave handwriting at stale physical coordinates. This refusal is a temporary implementation boundary, not an owner confirmation requirement or completion of Q071. Source-coordinate cell layout and per-capture mappings, ink-preserving structural changes, resizing, cell move/copy, scene/export integration and protection/resource coverage remain required. No production Android H.I. or spreadsheet calculation is introduced.
