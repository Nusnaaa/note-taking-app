# PDF source-policy and isolated-parser pipeline — 23 October 2026

Validated source: `8e017702eaa72a4add3931e8c19b752b81ec9c1f`, `phase-a/backend`.
Desktop and Android source fingerprints match, with no uncommitted non-evidence
inputs. Format, strict workspace Clippy, **145 Rust tests** (no failed/ignored),
release build, traceability, ARM64 compilation/ELF, **seven actual desktop JNI smoke
groups**, **37 Android unit/Robolectric tests** (no failures/errors/skips), Android
lint, AAR native payload and the specific isolated non-exported PDF service manifest
checks passed. Exact commands, hashes and logs are in
`continuation-2026-10-23/pdf-policy-pipeline/` and `latest/`.

Backend acceptance remains NOT MET; production H.I. remains gated. The backend gate
still exits 2 on substantive open groups and incomplete canonical/inherited semantic
coverage. All 94 requirements, 103 inherited clauses and 158 historical candidates
remain preserved; component test totals do not close their complete assertions.

## Delivered contracts

- Native JSON v1 PDF source/view/anchor/revision workflows now use the authoritative
  Store. Private bounded read leases and single-use revision reviews cannot be
  manufactured by deserialising requests. Relock/access/containment/source changes
  retire prepared capabilities. Current source/revision/access are rechecked on
  read validation. Ordinary pen edits and PDF view updates do not invalidate reads.
- Read validity is bounded by both the lease and remaining native authentication
  lifetime. Already-expired access is normalised before requests; an expiry racing
  authorisation must never become a fresh public lifetime. Cached frames enforce
  the conservative monotonic deadline and reject drawing after retirement.
- `NotebookBackend.renderPdf` connects this policy to bounded provider opening, a
  streamed hash-verified private snapshot and an isolated parser client. The live
  cache is unlinked before parsing, never becomes a managed library copy or export,
  and is removed on interruption/startup. Copying has bounded buffers, a 16 GiB
  operation limit and an 8 MiB disposable-copy free-space guard; these are engineering
  resource bounds, not measured device performance or a zero-loss guarantee.
- The client binds to a fresh isolated service instance, tracks request identity,
  Binder death, timeout, cancellation, late binding/replies and owned descriptors.
  Pending capacity is retained until outstanding operations/disposal actually retire.
  The parser control thread remains separate from parsing/pipe work. Termination
  sends the stop control message and unbinds; an isolated service also terminates
  on destruction. This is an implemented lifecycle contract, not a measured device
  termination-latency claim.
- Only read-only FD/page/viewport/token/format data reaches the worker. No source
  locator, notebook path, document ID/title, authentication token or native Store is
  supplied. Provider access is reopened read-only, and native policy is validated
  again after parsing before a viewport can be published.
- Private IPC `stem-argb-1` sends an exact versioned header and bounded big-endian
  ARGB rows with mandatory EOF. Invalid dimensions, malformed/truncated/trailing
  frames fail. The notebook process does not run an image decoder on worker output.
  This raster viewport is for PDF display, never handwriting recognition.
- Policy-changing backend requests retire pending/cached frames before enqueuing
  the native change. Ordinary captured pen observations remain unchanged. Old
  receipts cannot retire newer frames. `closeAsync` acknowledges native writer
  drain; uncooperative platform callbacks may return later and cannot publish.

## Executed boundary evidence and limitations

Seven new Rust host fixtures cover private/bounded tokens, stale and rejected
reviews, restart/relock/auth changes, source revision/anchor lineage, revocation,
independent authored work and exact source-ink preservation. The Android tests add
raw-pixel framing, simulated Messenger transport, late binding/closure races,
blocked-send backpressure, malformed/dead workers and actual desktop JNI pipeline
fixtures. They exercise frozen source hashes, permission loss before publication,
revocation during final validation, cached-frame expiry, old receipts, low storage,
retirement on relock/revoke and a measured negative-zero pen sample after persistence.
The JNI runner keeps the pure Java bridge in the parent classloader; it does not
replace the native core with a Robolectric shadow. Providers/parser output are
synthetic fixtures and establish no handwriting/model accuracy or PDF fidelity.

The first exploratory compilation rejected local Kotlin property references in
fixtures; they were replaced with explicit latches. A subsequent 33-test attempt
had four failures caused by the transport fixture duplicating an empty simulated
pipe before writing. Inspection confirmed Robolectric implements pipe/dup with
regular files and copies. Correcting the fixture yielded 33 passing methods/lint,
then the expanded 37-method suite/lint passed and full exact-source validation above
passed. No failed attempt was counted as passing. These fixtures do not establish
live kernel-pipe or actual Binder cross-process/device behavior. Context-wrapper
binding checks establish requested API/flags/cleanup, not an OS isolation benchmark.

Still incomplete: trusted picker/regrant/restore (ordinary JSON cannot supply a
platform-grant flag), managed-copy/historical-snapshot render paths, complete platform
permission/lifecycle/navigation/annotation workflows and comprehensive interruption,
malformed/resource acceptance. Real isolated-service/device and H.I. pixels remain
separate post-gate acceptance; already drawn external canvases must be hidden/redrawn
on retirement by the eventual H.I. All other backend gap groups remain open too.
No Activity/Compose screen, proprietary weights/font binary, handwriting upload,
remote inference, implicit cloud backup or push to origin has been introduced.
