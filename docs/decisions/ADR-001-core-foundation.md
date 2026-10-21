# ADR-001 — Authoritative Rust core and SQLite transactions

22 October 2026. Engineering selection within the approved architecture strategy.

One Cargo workspace contains the portable Rust core; Android host and later H.I.
belong to this same repository. Rust 1.90.0 is pinned, with Cargo.lock recording
resolved dependencies. KMP remains a future alternative if measured integration
cost warrants it; no Rust-versus-KMP speed claim is made. There is no production H.I.

SQLite transactions own organisation, immutable stroke observations, separate
editable block representations, history and proposal records. A single Store owns
a connection; distinct connections use SQLite write locking plus expected-revision
checks. WAL and FULL synchronous commits acknowledge SQLite's commit boundary.
Current implementation stores complete block snapshots for history, not whole
notebooks per edit; incremental source-ink optimisation remains future engineering.

UUIDs identify entities independently of content, title and location. Source pen
observations retain raw coordinates and explicitly named units, monotonic relative
microseconds and optional channels. No assumed millimetre calibration. JSON uses
float-roundtrip support, rejects unknown fields, and is never a rendering image.

Access capabilities are non-serialisable and expire after five minutes (engineering
default to validate with the host). Authentication is delegated to a trusted host
trait. This is NOT a security boundary against malicious code already inside the
application process, nor an implemented Android biometric integration. Models get
snapshots and no Store. Device host isolation still requires implementation/testing.

The repository does not assign an open-source licence to the owner's code. Third-party
code is obtained by Cargo and must be reviewed separately before redistribution;
no model weights or proprietary SDK are included.
