# Native subset v3: technical data, assets and views

The current encoder emits payload version `3`; the decoder accepts `1`, `2` and `3`.
This extends `native-subset-v1.md` and `native-subset-v2.md`. Wrapper checksum and
portable encryption remain unchanged. Unknown fields/versions fail closed. A v1/v2
payload cannot contain nonempty v3 attachments.

`Content` additionally accepts tagged `Table`, `Diagram`, `Graph` and `Image` data.
The authoritative serialised structures are in `core/src/objects.rs`. Table cells
have stable IDs and text/numeric/maths values; numeric spelling is literal. Diagram
parts have IDs, labels and polyline/rectangle/ellipse geometry. Graphs have labelled,
optionally unit-bearing finite axes and ordered, identified series. Images refer to
an asset ID and carry a caption and optional normalised crop. No values are solved,
sorted, fitted or silently corrected.

The new `attachments` object has these arrays:

| Field | Records |
| --- | --- |
| `assets` | `id`, `entity`, `mime`, exact `bytes`, SHA-256 `digest` |
| `layouts` | `block`, `layout` with finite `bounds`, `z`, `revision` |
| `groups` | `id`, `entity`, `members`, `revision` |
| `embeds` | `id`, `host`, `source`, independent `camera`, `frame`, `pinned`, `revision` |
| `views` | `id`, `entity`, optional `exercise`, `camera`, `pen_width_mm`, `colour_rgba`, `finger_ink`, optional `anchor`, `revision` |

Validation checks identifiers, owners, references, finite geometry, asset hashes and
signatures, groups, view membership and embed cycles before writes. Import maps all
durable asset/group/embed/view identities alongside entity/block/stroke/exercise IDs.
Image references in retained history are remapped too. Local component IDs remain
stable inside their independent content objects. Images are capped at 32 MiB each;
aggregate assets at 64 MiB, with the existing 64 MiB encoded-envelope limit applying
as well. Byte-array JSON overhead reduces usable payload size. These limits do not
claim large-notebook scalability or image-decoder safety.

Only selected blocks' current or historical image references are included. Groups
that cross the selected boundary are omitted with a notice. A whole-host embed
requires explicit selection of its source canvas too; export never automatically
includes an entire linked canvas. Selected-exercise exports omit unanchored master
embeds and retain only relevant working views, converted to independent-note views.
Fresh protected-content authentication and library/generation checks bind the review
to its original Store and scope.

This is still a subset format. Source/PDF records, pending proposals, Trash, global
settings, membership undo operations and complete import lineage are not included.
Source-linked documents are refused. Use the separate encrypted whole-library
snapshot for current-schema library recovery, not a subset copy.
