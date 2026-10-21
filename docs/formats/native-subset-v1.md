# Native subset v1 and portable encryption envelope v1

This is an implementation format for the current core subset, **not the full
approved native format or a comprehensive backup**. The decoder rejects a future
version. Migrations to the complete native format must preserve these files.
Source-linked documents currently cannot be exported through this subset API.

## JSON wrapper and primitives

UTF-8 JSON object: `{"payload": "<JSON string>", "sha256": "<lower-case hex>"}`.
The SHA-256 covers the exact UTF-8 bytes of the decoded `payload` string, not the
wrapper or an independently reserialised object. This checksum detects damage;
it does not authenticate a plaintext file. Structural validation is mandatory even
if a sender recomputes it. Unknown wrapper/document fields fail decoding.

The payload has exactly these fields:

| Field | Type / meaning |
| --- | --- |
| `version` | integer, currently `1` |
| `created_ms` | signed 64-bit Unix epoch milliseconds at export |
| `entities` | ordered array of Entity |
| `blocks` | ordered array of NativeBlock; sequence determines imported block ordering |
| `links` | array of Link |
| `omissions` | array of human-readable boundary omission strings |

IDs are UUID strings. Entity, block, stroke and link IDs must be unique across those
collections. A capture session may be shared by many strokes. Maths-node IDs are
unique within their tree and remain stable through local subtree editing. Integers
must be represented exactly; consumers must not route u64 values through IEEE-754
double arithmetic. Coordinates are finite binary64 values; preserve their exact
roundtrippable values and order. Optional fields encode absent values as `null`,
which differs from zero, empty string and an actual measurement.

## Stored structures

| Structure | Fields |
| --- | --- |
| Entity | `id`, `kind`: `Module/Project/Notebook/Note/Canvas`, `parent`: ID or null, `title`: string, `created_ms`, `modified_ms`: i64, `revision`: nonnegative integer within i64 range, `locked`, `trashed`, `purged`: booleans |
| NativeBlock | `current`: Block, `strokes`: Stroke array, `history`: HistoryEntry array |
| Block | `id`, `entity`: IDs; `revision`; `content`: Content; `source_ids`, `visible_strokes`: ordered ID arrays; `transform`: Transform; `display`: `Original` or `Editable`; `visible`, `locked`, `manually_corrected`: booleans |
| HistoryEntry | `revision`: matching block revision; `active_ms`: nonnegative active-use time; `block`: full historical Block |
| Stroke | `id`, `capture_session`: IDs; `sequence`: u64; `pointer_id`: u32; `coordinate_space`: nonempty string; `tool`: `Pen`, `Eraser` or `ExplicitFinger`; `points`: ordered nonempty Point array |
| Point | `x`, `y`: f64; `time_us`: u64 monotonic relative microseconds within named session; `pressure`, `tilt_rad`, `orientation_rad`: f64 or null; `buttons`: u32 or null |
| Transform | `a`, `b`, `c`, `d`, `tx`, `ty`: f64; derived affine presentation x'=a*x+c*y+tx, y'=b*x+d*y+ty; invertible |
| Link | `id`, `origin`, `target`: entity IDs except its own link ID; `anchor`: origin block ID or null |

Original points are observations, not predictions or calibrated millimetres.
Transforms and erasure visibility never overwrite them. `source_ids` must exactly
match the stroke array's ordered IDs; visible IDs must be a unique subset. Historical
blocks use the same owner/ID, cannot invent source strokes, cannot exceed the current
revision and cannot repeat a revision. A selected external parent is detached only
with an omission notice; unselected link targets are omitted and disclosed.

All entities in this subset must be live. Parent references must resolve inside the
payload and cannot cycle. Root allows all five entity kinds. Module permits any
kind but Module nesting is at most two levels; Project permits Notebook/Note/Canvas;
Notebook permits Note/Canvas; Note/Canvas have no children. Blocks belong only to
Note/Canvas. Links must resolve inside the payload, with an optional anchor belonging
to the origin. Limits include 64 MiB encoded input, 10,000 entities, 100,000 blocks
and 100,000 samples per stroke. These are defensive limits, not product SLOs.

## Editable content

Content uses tagged JSON: `{"type":"Text","value":"exact text"}`.
Types are `Ink` (no value), `Text` and `Numeric` (strings), `Code` (value object with
`text`: string and `language`: string or null), and `Maths` (MathNode). Numeric
content remains a string, preserving leading zeros and the author's mistakes.
Code whitespace and line endings are not normalised by storage.

MathNode is `{"id":"UUID","form":{...}}`. Form uses `type`/`value` tags:

| Form type | Value |
| --- | --- |
| `Token`, `Text`, `Unknown`, `Unparsed` | exact string |
| `Row`, `AlignedLines`, `Alternatives` | MathNode array |
| `Fraction` | object: `numerator`, `denominator` MathNodes |
| `Root` | object: `radicand` MathNode, `index` MathNode or null |
| `Scripts` | object: `base` MathNode, `sub`, `sup` MathNode or null |
| `Fence` | object: `open` string, `body` MathNode, `close` string or null |
| `Matrix` | array of rows, each a MathNode array |
| `Hole` | no value |

This represents presentation and incomplete/incorrect notation; it is not a solver
AST. Missing closing fences, unknown tokens and alternatives must not be repaired
silently. Current structured editing and conversion are limited to these forms.

## Import semantics and limitations

Decode, checksum and structural validation precede all writes. Import is one
transaction, makes independent entity/block/stroke/link IDs, preserves creation times,
adds “(recovered)” to titles and retains protection flags. It does not overwrite an
existing document. Destination platform credentials must be established separately.
Retained history ages translate to the receiving active-use clock and may clamp at
zero; this is not a lossless history-age transfer. Math-node/capture-session IDs stay
as provenance within the independent content. Full import lineage mapping is open.

Missing from this subset: proposal history, conversion modes, global settings,
complete operation undo/redo, source/PDF records and bytes, retained evidence assets,
Trash and future rich objects/exercise/camera state. A source-free subset export
does not become comprehensive merely because it is encrypted.

## Encrypted envelope

All offsets below are byte offsets with exclusive ends. No integer fields or
platform credentials are stored in this version. AEAD is XChaCha20-Poly1305 with
32-byte key, 24-byte nonce and 16-byte appended authentication tag.

| Offset | Length | Contents |
| --- | ---: | --- |
| 0–8 | 8 | ASCII `STEMENC1` |
| 8–24 | 16 | random password salt |
| 24–48 | 24 | password wrap nonce |
| 48–96 | 48 | 32-byte data key encrypted with password key, plus tag |
| 96–120 | 24 | recovery wrap nonce |
| 120–168 | 48 | same data key encrypted with recovery key, plus tag |
| 168–192 | 24 | payload nonce |
| 192–end | plaintext length + 16 | encrypted UTF-8 wrapper, plus tag |

Password key: Argon2id version 0x13 over the exact UTF-8 password (no normalisation),
salt bytes 8–24, memory 65,536 KiB, time cost 3, parallelism 1, output length 32.
Both key wraps use bytes 0–24 as AEAD associated data. Payload encryption uses the
entire 192-byte header as associated data, including both wraps and all nonces.
Each data key, recovery key, salt and nonce is independently generated from OS RNG.
The recovery key is encoded as 64 hexadecimal characters and returned separately;
it is never placed beside the encrypted payload automatically.

Opening either route verifies its wrapped key then the payload authentication tag.
Unknown magic, too-short/too-large input, wrong credentials, corruption and truncation
fail. Maximum plaintext is 64 MiB; maximum envelope is that plus 208 bytes. Readers
must not act on plaintext until both verification and native structural checks pass.
Never log keys/passwords or silently fall back to a device key or cloud service.

Automated tests exercise this implementation. Cross-implementation interoperability,
full format migration, independent security review and actual-device resource
measurements remain open. No claim of audited or production-complete backup is made.
