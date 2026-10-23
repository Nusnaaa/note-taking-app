# Retained PDF acquisition and rendering — 23 October 2026

Source checkpoint: `afbd5c2`, branch `phase-a/backend`.

Backend acceptance remains NOT MET; production H.I. remains gated. This increment
adds explicit native retention and backend-only retained rendering; it does not
close the complete PDF assertion or other open implementation groups.

Full exact-source validation passed with matching clean source manifests:
**158 Rust tests**, **51 Android unit/Robolectric tests**, and **eight desktop JNI
smoke groups**, with no failures/ignored/skipped tests. Format, strict workspace
Clippy, release build, traceability, ARM64 compilation/ELF, Android lint, AAR payload
and isolated non-exported service manifest checks passed. Commands/hashes/logs are
in `continuation-2026-10-23/retained-pdf/` and `latest/`.

## Delivered contracts

Native PDF retention has explicit class/consent/independent-authority/export choices,
a single bounded private transfer, exact sequential chunks, and a confirmed final
transaction. No partial asset is written into SQLite or backups. Transfers are
capped at 64 MiB per host, chunks at 65,536 bytes and lifetime at five minutes
(retired on subsequent host work). Allocation failure, invalid chunks, stale source,
relock, rejection and wrong length/signature/digest fail closed. Final publication
requires the exact currently linked revision. Ordinary note edits can proceed
between chunks. These bounds are engineering controls, not measured tablet SLOs.
Independent snapshot authority is an explicit recorded decision, not a right inferred
from an earlier provider grant or a document being old.

Retained asset metadata and bounded SQL BLOB chunks enforce current context locks,
asset class, authority and source revision. Disposable live caches are ineligible.
Live and retained reads share the 32-capability/lifetime budget. An unavailable
external provider does not remove a managed local copy, while source revocation
purges copies without independent authority. Separately authorised historical
assets retain captured revision/provenance until their own retention is revoked.
Source revision replacement never relabels old retained evidence as the new edition.

`NotebookBackend.renderRetainedPdf` reads the checked chunks into its private
disposable cache without opening a provider. It validates offsets/counts/ranges and
the complete digest, unlinks the frozen file before isolated parsing, and validates
native policy again before publishing. Frames identify the retained asset/captured
revision and independent historical status. Relock, source/retention policy changes,
expiry or late cancellation retire the private bitmap. No bitmap/export capability
escapes the frame. Original ink and independently authored notes remain untouched.

## Executed fixture scope

New native host fixtures cover explicit/non-mutating staging, wrong chunks/bytes,
replay/release/relock, bounded shared read admission, exact chunk reconstruction,
source revocation versus independent authority, restart, source-edition provenance
and locked metadata/content. An injected SQLite commit failure proves no retained
asset or acquisition change is published and authored work remains intact.

Actual desktop JNI Android fixtures cover a managed PDF larger than one native
chunk while the source is missing, historical rendering after live revocation,
retention revocation after publication and during final validation, and corrupted,
dropped, misaddressed and out-of-range chunks failing before parser admission.
Provider opens are asserted absent for retained reads. The first exploratory Rust
fixture compared independently generated stroke UUIDs; retaining the captured
fixture value corrected that assertion before its passing rerun. No failed result
is counted as successful evidence.

Providers, transport and parser output remain synthetic. Robolectric pipe/dup
behavior is file-backed rather than actual kernel pipes. These checks do not prove
real Android Binder isolation, parser fidelity, device resource behavior, model
quality, final app privacy or H.I. acceptance. The typed Android picker-to-retention
acquisition workflow, complete external permission/lifecycle/navigation workflows,
and other backend gap groups remain open. No production H.I., model weights,
handwriting upload, cloud inference, automatic OS content backup or origin push is
introduced. `handover-do-not-commit/` remains excluded and untracked.

The backend gate exits 2 for substantive open groups and incomplete canonical/
inherited coverage only. No stale or mismatched automated-evidence reason remains.
