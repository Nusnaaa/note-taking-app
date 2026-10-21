# ADR-002 — Explicit portable subset and trusted host boundary

22 October 2026. Engineering choices within the owner-approved Rust/SQLite strategy.

The initial native serializer is named a **subset**, not a comprehensive backup.
It preserves current entities, editable blocks, original strokes, retained block
history and selected internal links. It refuses documents with linked PDF sources.
Proposals, modes, settings, source assets, Trash and future exercise/canvas state
require the next format increment and full roundtrip coverage. Until then the
complete-backup acceptance obligation is open. No lossy subset is advertised as a
safe replacement for the owner's future library backup.

Exports use an explicit exact-entity selection without following links/children.
Ink inclusion and boundary omissions require acknowledgement. Protected content
requires a fresh trusted-host authentication call for both encrypted and plaintext
paths; plaintext additionally requires deliberate acknowledgement. Generation checks
before/after snapshot, authentication and encryption reject concurrent mutation.

Portable encryption uses the RustCrypto Argon2id and XChaCha20-Poly1305 libraries.
Each file has a random data key, independently wrapped by a password-derived key and
a random recovery key. The fixed Argon2id profile is 64 MiB, three iterations, one
lane; this is an implementation choice, **not** a claimed device benchmark or exact
RFC recommended profile. A credential-length floor does not guarantee entropy.
The custom envelope has automated integrity/recovery tests but no independent audit.
Its complete byte layout is in `docs/formats/native-subset-v1.md`.

Primary references consulted for these choices: [RFC 9106](https://www.rfc-editor.org/rfc/rfc9106.html),
[RustCrypto Argon2](https://docs.rs/argon2/0.5.3/argon2/),
[RustCrypto ChaCha20-Poly1305](https://docs.rs/chacha20poly1305/0.10.1/chacha20poly1305/).
An upstream library audit is not an audit of this application's envelope or host.

The app database is not independently encrypted by current content locks. Locks
are backend authorisation checks within a trusted application process. Rust traits
are not a sandbox against malicious adapters; a real native SDK needs reviewed
licensing, bounded worker isolation, egress enforcement and Android integration.
Cancellation currently prevents commit of late results but cannot forcibly stop
a non-cooperative worker. No data-upload API, downloader or remote inference client
is present. A built-in recogniser has not been selected or licensed. Missing models
do not prevent ink storage.

Android platform authentication, Keystore lifecycle and explicit OS cloud-backup
and device-transfer exclusions remain gate-blocking host work. There is no Android
manifest yet, so no claim that a built Android application enforces those exclusions.
