# Native subset v12 — explicitly scoped cell targets

V12 extends v11's region object with optional `cell: UUID`, omitted when absent.
`id` remains the globally unique ink-region identity, stable through retained
revisions of its owning block. `cell`, when present, is local to the containing
table and must identify an existing cell. Different tables may reuse cell IDs,
including values equal to global object IDs. Regions in one table cannot target
the same cell twice. Source-stroke membership remains nonempty and disjoint.
Cell bindings on non-table content and mismatched accepted cell values are invalid.

New cell captures create a fresh region ID and explicit cell target. V11 aliases
without `cell` keep using their region ID as their cell target; old payloads and
receipt hashes are not rewritten. Public table selectors (`InkSnapshot.region`,
`Proposal.region`, cell capture and raw-cell bookmarks) use the table-local cell ID.
A freehand selector uses the region ID. The block supplies the scope in both cases.
Independent copies remap global region IDs and local cell references together.

Writers emit v12. The new field under an older native header is unsupported,
including in retained history. SQLite schema20 fences the semantic extension only;
no physical tables/columns are added. JSON backups write20/read9–20; streaming
writes20/reads15–20, with unchanged physical table indices. Sources below schema20
must not contain explicit cell targets in current/history/Undo rows. V11/schema19
aliases retain their prior meaning and original observations remain independently
owned by their source block. Q071 sizing/row/column workflows remain separate.
