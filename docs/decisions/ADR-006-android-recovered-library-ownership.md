# ADR 006 — Android recovered-copy ownership and activation

25 October 2026. Engineering decision within the approved Rust/SQLite/Android
strategy; production H.I. and complete backend acceptance remain gated.

The restore workflow belongs to NotebookBackend and an optional worker, with one
review/native job per controller. JNI transport alone cannot own provider opening
before native admission or acknowledgement/cleanup after a native result. Full app
close therefore waits for the controller, cancellation listeners and descriptors
before releasing LibraryOwnership. Input observation/interruption does not release
work that is still running. An uncooperative provider can keep close pending; it is
not claimed hard-terminated. Idle native stage disposal runs off the authoring and
control thread.

A deliberate read picker provides an encrypted input URI. Current OS read grants
are independently checked; no persistable permission is taken automatically.
Credential copies are bounded and cleared, and only ciphertext reaches the input
adapter. The caller keeps ownership of its original credential. Source grants are
checked before/after reads and around the fresh publication verifier. Access and
containment policy changes invalidate idle review capabilities.

Recovery creates a fresh UUID root under app-private no-backup storage. It does
not overwrite the active root, reconcile or discard capture journals, or activate
itself. Existing protection flags, source ink and grant/runtime retirement remain
core restore responsibilities. Fresh confirmation and proof precede native
no-overwrite publication. An entered publication always yields its possible-copy
receipt rather than disappearing behind observation cancellation. Post-publication
ambiguity keeps the root and an Uncertain result.

A synced, bounded metadata marker identifies a recovered root and supports paged
restart discovery. It records snapshot counts and publication state, never source
content, provider grants or credentials. Explicit opening verifies the existing
marker and SQLite file and starts an independent backend. A missing copy cannot
silently become a new empty library. Discovery records are historical metadata,
not a new durability acknowledgement or a grant to protected content.

Caller-specified input/plaintext and free-space bounds are engineering controls,
not device SLOs or a hard native SQLite staging quota. Unknown/orphan staging is
preserved, reported and blocks new preparation. Normal cleanup deletes only an
empty exactly owned directory. Explicit orphan review/cleanup, hard staging quota,
bounded whole-graph semantic validation, malformed/storage interruption coverage
and real-provider/device acceptance remain open. No UI, automatic cloud backup,
remote inference or model installation is introduced by this decision.
