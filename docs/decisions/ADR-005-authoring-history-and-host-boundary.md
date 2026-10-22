# ADR-005 — authoring history, local host and PDF output

22 October 2026. Engineering decisions within authorised Phase A; product scope and
owner approval are unchanged. Full backend acceptance remains NOT MET.

The core remains Rust/SQLite. Schema 9 adds transaction-local edit journalling for blocks,
layouts, groups and embeds. Original strokes are never journal-deleted. Undoing creation
hides the block; redo reveals it. Replay advances revisions, checks current permissions
and preserves current locks. A later edit invalidates the redo branch. Group/selection
edits replay atomically. Organisation/security/source-policy operations are deliberately
not accidentally reversed through this authoring journal. Their complete operation-history
coverage is still open. Active-use pruning preserves protected histories when the caller
cannot authorise their blocks. Logical backup schema 8 upgrades to 9 with empty undo logs;
it does not invent historical actions.

Native subset v4 adds independent selected-stroke regions with explicit, revision-safe
recognition acceptance. Regions preserve original source ownership. Repartitioning accepted
content is refused pending a deliberate preservation/edit workflow. No raster recognition,
solver, handwriting upload or accuracy claim has been introduced.

The host owns a bounded single-writer thread. JSON protocol version 1 uses typed commands,
content-free failures and durable acknowledgements. Android's JNI library uses opaque
non-reused handles, a private registry and panic containment. It passes no database pointer
or authentication JSON command to model code. Platform authentication is a trusted Android
BiometricPrompt/device-credential callback, with a short-lived, single-use proof; the core
still checks scopes. This is the approved Android/device lock baseline, not extra database
encryption. In-process arbitrary malicious code is not a defended boundary; actual
untrusted engines need the outstanding process-isolation integration.

Android is a backend library module in the same repository, with no Activity, Compose screen
or production H.I. The manifest excludes cloud backup and device transfer and removes
INTERNET permission. Final application manifest/device enforcement still needs verification.
MotionEvent integration consumes observed historical/current coordinates, explicitly handles
cancellation and ignores fingers in pen mode. Missing channels remain null. Android may
normalise values before exposing them; losslessness begins at the actual observed API.
The PDF provider opens only explicit content URIs read-only. Its parser service runs under
an isolated UID with a responsive control thread separate from parsing. End-to-end provider,
revocation, worker-timeout and device tests remain outstanding.

Local resource installation records a consented descriptor and verified hash, resumes at
its durable receipt, rejects changed retries, and publishes only complete matching bytes.
Shared resource leases prevent cleanup while a worker owns a file. The host must use one
shared lease registry; full integration with every installed-engine path remains open.
Converters have durable staged/probed activation, with failed-update rollback. No real
recogniser or proprietary model is installed.

PDF output uses svg2pdf/pdf-writer with Rustybuzz measurements and explicitly supplied
local fonts. It emits vector maths and embedded selectable text on paginated metric page
sizes; it does not evaluate expressions. Unsupported glyphs and unsupported image/embedding
fidelity fail explicitly. Font binaries are not bundled. Synthetic output was inspected with
Poppler; that does not establish full document/export acceptance or all script/font coverage.

Dependencies are pinned in Cargo.lock. Registry declarations and licence-text hashes are
recorded separately. Permitted routes now include MIT, Apache-2.0, BSD and Zlib as actually
declared; the old inventory's blanket MIT fallback is removed. Source publication and any
future binary/model redistribution remain distinct checks. No commercial requirement was added.
