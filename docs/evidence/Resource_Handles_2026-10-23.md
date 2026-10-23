# Guarded resource reader lifetimes — 23 October 2026

Source checkpoint: `af80108`, branch `phase-a/backend`.
Backend acceptance remains NOT MET and production H.I. remains gated.

## Reproduced problem and fix

ResourceLease.file previously returned a raw cloned File. Dropping the original lease
removed the pool reservation while that worker-owned descriptor was still alive, so
ResourceLeases.remove incorrectly succeeded. A scratch reproduction under /tmp first
observed this; an actual repository regression then failed on the expected Conflict
assertion before the fix. The original cargo invocation/output is retained in
continuation-2026-10-23/resource-handles/regression-before-fix.log. Its source baseline
was fb52a22 with only the new failing fixture added; it is development regression
evidence, not clean-source acceptance.

ResourceLease.file now returns another owned guarded ResourceLease implementing
Read/Seek. Successful nested clones increment the same shared pool reservation and
keep it alive after ownership transfer to another thread and after the original lease
is dropped. Clone failure does not reserve a handle. Reader field/drop ordering closes
the descriptor before releasing its reservation. Pool removal continues to conflict
until the final reader retires, then succeeds. Counters use checked increments.
Cloned descriptors retain File::try_clone's shared seek position; callers coordinate.

The Rust return type changes deliberately, replacing an escaping owned File with a
guarded reader; there were no production callers of the old accessor. No raw owned
file/descriptor extraction is introduced. Workers must share the same pool; this does
not yet wire recogniser adapters, mapped native resources, scheduling or isolated
process termination into the host. Those integrations remain open in A4.

## Executed coverage

Two new native regressions cover original-owner drop with a live worker reader,
Read/Seek, nested clones, actual thread ownership transfer and removal only after the
last worker retires. All six provisioning tests and strict workspace Clippy passed in
development. Complete frozen-source validation passed on matching clean source manifests at af80108:
**232 Rust tests**, **129 Android tests**, ten desktop JNI smoke groups, format,
strict Clippy, release, traceability, ARM64/ELF, Android lint, AAR payload and manifest
checks. No tests failed, were skipped or ignored. Raw commands, source hashes, timings
and output are archived in resource-handles/ and latest/. E01 did not recur in this
run; its cause remains unestablished and its follow-up remains open.

## Boundaries

No model installation/download, SDK/weights bundling, executable plugin loading,
handwriting egress, network permission or production H.I. was introduced. Notebook
source ink, receipt encodings, SQLite/native/backup formats and existing capture routes
are unchanged. This component test is not a device performance or full worker-cleanup
acceptance claim. The prior E01 PDF fixture timeout remains an open follow-up; no
timeout/assertion was weakened or test disabled.

The handover folder remains ignored/untracked and excluded from commits and pushes.
