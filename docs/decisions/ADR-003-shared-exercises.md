# ADR 003: shared exercise membership and protection

Status: implemented core increment; full backend acceptance remains open.
Authority: approved v0.5.8 NB-DOC-003, NB-DOC-004 (I1 independent copies),
NB-DATA-008, NB-REC-010 and NB-SEC-001. This records implementation choices,
not an amendment to approved behaviour.

## One logical source

An Exercise has its own stable identity and a relation to a master Note. It is not
a child Note in the organisation hierarchy. A membership table maps a block to at
most one exercise; the block, observations and revision history remain in the
master. Both views call the same revision-checked edit APIs. Appending through a
linked exercise inserts after its final member in master order and commits source
ink, membership, block history and the master's revision in one transaction.
An empty exercise appends at the master end. This does not establish host caret or
scroll behaviour, which still requires the future application integration.

Confirmed text headings delimit extraction candidates. The configurable keyword
matches an exact prefix followed by whitespace or the end of the heading; default
is `Exercise`. Incidental prose never starts a region. A preview only proposes
previously unassigned blocks, ending at the next confirmed heading. Acceptance is
explicit and bound to the database generation. Heading edits or confirmation
removal do not change membership. Duplicate titles are allowed and have distinct
identities. View titles have an explicit rename operation; editing a heading does
not implicitly rename or reassign the view.

Removing a view sets its visibility false. Its membership, protection and master
content remain and the same view can be shown again. Master Trash/restore preserves
exercise state; separately authorised permanent deletion removes it transactionally.
Deletion review reports exercise dependencies, including hidden views.

## Reassignment and access

Reassignment requires a generation-bound preview and explicit confirmation. Each
block has a separate membership version, including while unassigned. Undo checks
both membership and version, then writes a new inverse operation. It preserves
intervening content edits and rejects stale history, including an A-to-B-to-A cycle.
Undoing the returned inverse operation can reapply membership while the version
still matches. This API is not a general application undo/redo stack.

An exercise lock protects its members through the master, raw-ink retrieval,
history, recognition snapshots, search, anchored links, copying and export. Hiding
or changing a heading cannot evade it. Moving a block out of a locked exercise
carries protection onto the block; undo never silently declassifies that block.
An independently locked heading produces a locked extracted view so its title
cannot leak through the directory. Newly appended exercise content is protected by
its durable membership. Empty and hidden locked exercises require access too.

Copy/export authentication and generation checks apply to the selected scope.
Protected export needs fresh authentication for encrypted and plaintext output.
Trash rechecks content access at commit, since authentication may expire after
review. These are core authorisation checks. They do not encrypt the SQLite file
or substitute for Android authentication, secure previews or process isolation.

## Shared persistence state and independent copies

The trusted host must own one Store and route both views through it. Per-document
save state is keyed by master identity, so linked views share writing, proposal,
heading, reassignment, exercise metadata and direct master metadata failures.
Independent copies have new entity, block, stroke and link identities, separate
state and a snapshot of the effective writing mode. Later edits and mode changes do
not propagate between copy and original. A linked exercise always reads the current
master mode. Stored source observations remain unchanged.

Pending/failure state is currently in memory in that Store. Reopening reports the
committed local state; separate Store connections do not share transient failure
status. Durable revisions prevent stale overwrite between connections. `Saving` is
internal to synchronous writes, so a nonblocking host subscription/queue is still
needed to display in-progress saves. Local success never means a backup completed.
Container-wide, source-provider and global-setting save-status aggregation remains
unfinished. A future host must retain/retry pending user edits after failed writes.

## Portable boundary and remaining work

Native subset v2 preserves master/view relations, current membership, heading
confirmation, keyword and effective modes. It reads v1 files. Selected exercise
export includes only member blocks and eligible internal anchored links, with
unrelated master work excluded. Independent copying uses the same scope boundary.
See `docs/formats/native-subset-v2.md` for explicit omissions.

Remaining obligations include protected placeholder/navigation projections,
persisted working positions, flat exercise search, whole-operation history and
retirement, full backup, rich objects/assets, host scheduling and automated Android
integration. M1 linked-duplicate propagation remains deferred. No H.I., model
accuracy, pen quality or complete requirement acceptance is claimed by this ADR.
