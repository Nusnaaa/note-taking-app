# Native subset v2: exercise relationships and modes

The current encoder emits payload version `2`; the decoder accepts `1` and `2`.
This remains a subset export, not the complete native format or full backup.
The wrapper, source samples, editable content and encrypted envelope are unchanged
from `native-subset-v1.md`. Unknown future versions fail closed. Legacy v1 files
without these extension fields remain readable; nonempty extensions in a payload
declared as v1 are rejected.

The following arrays extend the document payload:

| Field | Contents |
| --- | --- |
| `exercises` | Exercise records: `id`, `master`, optional `heading`, `title`, `revision`, `locked`, `visible`, `created_ms` |
| `memberships` | Records with `block`, `exercise`, positive integer `version` |
| `headings` | Unique IDs of explicitly confirmed heading blocks |
| `rules` | Unique per-master records with `master` and nonblank `keyword` (at most 256 UTF-8 bytes) |
| `modes` | Unique records with `entity` and `mode`: `InkOnly`, `Manual` or `Assisted` |

Exercise identities must not alias any entity, block, stroke, link or other exercise.
A master is a Note in the payload; any associated heading belongs to that master.
At most one exercise refers to a particular heading. Each member block appears at
most once and belongs to the same master as its exercise. Revisions and membership
versions fit signed SQLite 64-bit integers. Heading, rule and mode references must
resolve. Malformed references fail before database writes, even with a recomputed
checksum. Import remaps identities and relationships atomically to independent IDs.

Whole-master export includes hidden views and requires access to locked views,
including empty ones. Effective writing modes are snapshotted so export without an
ancestor does not silently adopt the receiving global default. After import the
linked exercise reads its imported master's mode. Subset restoration does not
reconnect to any existing source document.

Selected exercise export produces one independent Note named after the exercise,
with only its member blocks, original samples when explicitly included, and their
retained block histories. Links are included only when anchored in selected working
and their targets remain within the selected entity boundary. No unselected master
title, adjacent content or other exercise view is added. Exercise protection is
carried onto the exported Note; block protection is retained. The export preview
reports included block count, raw-ink inclusion and omissions. Disclosure still
requires explicit confirmation and fresh authentication for protected content.

Independent local copy uses this selected scope, preserves editable values and
source observations, creates independent durable identities and a separate mode
override, and adds an independent-copy title suffix. It checks generation and
authorisation again at the write boundary. Subsequent edits do not propagate.

Omissions remain explicit: reassignment undo operations are not portable in v2
(an omission is reported when they exist); unassigned membership-version counters
are not imported. Proposal history, complete operation history, global settings,
source/PDF records and assets, Trash, future rich objects, cameras, host positions
and full import lineage are still absent. Source-linked documents are refused.
Historical active-use ages retain the v1 translation/clamping limitation. Neither
encryption nor successful subset roundtrip makes this a comprehensive backup.
