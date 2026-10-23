# Explicit Android PDF retention — 23 October 2026

Source checkpoint: `77e487d`, branch `phase-a/backend`.

Backend acceptance remains NOT MET; production H.I. remains gated. This increment
connects the typed Android acquisition workflow to the previously validated native
retention and renderer. Broader permission/lifecycle/navigation and all other open
backend groups remain incomplete.

Full exact-source validation passed with matching clean source manifests:
**158 Rust tests**, **64 Android unit/Robolectric tests**, **eight desktop JNI smoke
groups**, and no failures/ignored/skipped tests. Format, strict workspace Clippy,
release build, traceability, ARM64 compilation/ELF, Android lint, AAR native payload
and isolated-service manifest checks passed. Exact commands, hashes and logs are
in `continuation-2026-10-23/android-pdf-retention/` and `latest/`.

## Delivered behavior

Picker link/regrant acceptance remains copy-free by default. A deliberate second
`retainPdf` call requires explicit confirmation and a typed managed-copy or
historical-snapshot decision. Independent authority is separate from platform READ
permission, and export permission cannot be requested without that authority.
Failure leaves the accepted link intact; retries remain explicit and there is no
silent fallback to another acquisition/retention choice.

The adapter shares the bounded selection worker. It obtains a current native read
lease, freezes and hashes provider bytes off the writer, submits bounded native
chunks, unlinks the disposable private cache, reopens read permission and validates
native policy before submitting the final commit. No additional persistable grant
is obtained. Read validity is anchored before the native request, so delayed replies
cannot extend it. Copying is capped at 64 MiB, uses bounded buffers and an 8 MiB
free-space guard, and rejects oversized known descriptors before copying. Unknown
stream lengths remain bounded by counted bytes. These are engineering bounds,
not measured tablet performance or memory guarantees.

Cancellation before final submission prevents publication and releases staged
native data. Timeouts publish failure while retaining admission until late provider
resources retire. Once final submission begins, the adapter waits for its actual
receipt/error, avoiding false timeout/cancellation reports for a potentially saved
asset. A receipt contains source/asset IDs, not cached private locator/title data.
A failed ambiguous commit requires inspecting Assets after authentication rather
than automatically retrying and duplicating data.

Disposable files are removed on failure and startup; unrelated files are untouched.
A cache-disposal failure closes acquisition rather than admitting more copies.
Native notebook authoring remains independent. No original-ink representation or
native authoring implementation is changed by this Android increment.

## Fixture scope and limitations

Actual desktop JNI fixtures follow trusted picker link acceptance with explicit
managed/historical retention and retained rendering. They exercise no-copy defaults,
chunked acquisition, independent provenance, source/retention revocation, confirmation
and authority validation, changed bytes, permission failure at first/final open,
low storage/oversize, blocked-provider backpressure, authoring during optional waits,
relock, cancellation after native staging, cleanup and retry. Synthetic native seams
cover cancellation/deadline after final submission and expiry after delayed replies.
A filesystem fault replaces a disposable cache with a nonempty directory to exercise
failed-removal containment; this is not a physical storage failure benchmark.

The first exploratory 58-method run had three fixture JSON exceptions because the
helper treated null mutation responses as objects. The helper was corrected; its
60-method and expanded 63-method runs passed. Failed exploratory results are not
counted as passing. Providers, transport and parser output are synthetic; Robolectric
pipe/dup behavior is file-backed. Actual Binder isolation, kernel pipes, device
permission enforcement, parser fidelity, final APK privacy and H.I. acceptance are
not established by these desktop fixtures.

No production H.I., weights, handwriting upload, remote inference, implicit cloud
backup or origin push is introduced. `handover-do-not-commit/` remains ignored and
untracked. The full approved backend goal remains active.

The backend gate exits 2 for substantive open implementation groups and incomplete
canonical/inherited coverage only. No stale/mismatched/incomplete automated-evidence
reason remains.
