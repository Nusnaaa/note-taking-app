# Reviewed provider backup publication — 24 October 2026

Validated source: `c4df7abe83b76ed202e75f6f66cfa0b1fb50c1ff`, branch
`phase-a/backend`. Backend acceptance remains **NOT MET**; production H.I.
remains gated. This extends the Android backup controller with manual reviewed
provider-tree publication, not a complete scheduling/restore/device workflow.

## Implemented

Prepared.reviewProvider binds a deliberately selected SAF tree to that exact owned
encrypted artifact. Read/write picker bits alone cannot mint authority: the production
adapter checks current OS URI access, directory identity/type/create support and a
bounded display label. Publication requires confirmation and a fresh grant/directory
observation. No grant is automatically persisted and no backup capability comes
from ordinary notebook/model JSON.

The adapter uses the platform new-document API, checks the returned tree/authority/
document identity and child relationship, then opens only that deliberately created
document. It never accepts an arbitrary pre-existing output URI. The selected
provider is expected to honour Android's new-document contract; no guarantee of
provider honesty is inferred. Platform URI grants and creation are documented in
[Android DocumentsContract](https://developer.android.com/reference/android/provider/DocumentsContract).

Staged ciphertext is verified before external creation, copied in bounded frames,
checked again, flushed and closed, then reopened for exact size/SHA-256 readback.
Reliable descriptor errors and close failures prevent a verified result. Descriptor
sync is attempted; unsupported sync does not prevent successful byte verification.
No password, recovery key, original notebook path or decoded handwriting goes to
the provider adapter. Output filenames are random and contain no notebook title.
The URI receipt is for the trusted caller and has no generated URI-bearing toString.

| Outcome | Acknowledgement |
| --- | --- |
| Published | Existing local-file atomic no-overwrite/link and directory-sync workflow |
| ProviderVerified | Immediate provider readback exactly matches the prepared ciphertext; sync-call acknowledgement is reported separately |
| Uncertain | External creation/write may have occurred; inspect the new document before retrying; URI can be unknown if creation itself was ambiguous |

ProviderVerified does not promise atomic publication, durable remote commit, cloud
synchronisation or a live two-way mirror. No remote output is silently deleted or
retried after partial effects. The existing prepared artifact retains an explicit
recovery-key copy path until closed/invalidated; deliberately taken credentials or
external copies cannot be revoked. Source-current flags remain point-in-time local
epoch/native-generation comparisons, so an older valid snapshot does not claim to
cover subsequent authoring.

A remaining-deadline timer and per-operation CancellationSignal request cooperative
provider cancellation. Signal listeners run on a separate bounded worker, never the
control thread. New ordinary admission stays blocked until that callback actually
retires, even if payload work has already returned. Close awaits payload, cancellation
and timer workers before releasing app library ownership. There is no claim of hard
termination for an uncooperative provider. After external creation starts, cancellation
can drain to an explicit verified/uncertain result rather than hiding a possible
partial document behind a generic cancellation. Existing local commit semantics
remain unchanged.

## Frozen-source validation

Both validators terminated with exit 0 and finished timestamps, empty source working
tree and identical **218 source SHA-256 hashes** for the source revision above.
Actual hashes were rechecked after completion. Evidence is excluded from its own
fingerprint; full pre-run Git status is retained.

- **294 Rust tests passed**, zero failed/ignored; format, strict all-target Clippy,
  workspace release build and traceability passed.
- **167 Android tests passed**, zero failures/errors/skips. The **13 new provider
  fixtures** exercise fresh ciphertext-only output, supplied flags versus actual
  denied access, production adapter OS-denial, changed/revoked selection, corrupted
  staging/readback, ambiguous creation/non-child results, cooperative deadline,
  cancellation listener ownership/admission, writing during blocked readback, relock
  and simulated pipe/unsupported-sync behaviour. Existing controller regressions pass.
- **17 JNI groups passed before R8 and the same 17 after optimization**. ARM64/ELF,
  Android lint, debug/release AAR/native/consumer rules and packaged privacy manifests
  passed. No production Activity/Compose, permission or SDK/model dependency is added.
- Traceability retains 94 requirements, 103 inherited clauses and 158 candidates;
  no candidate PASS or whole-requirement closure is inferred. E01 did not recur;
  its original cause remains unestablished and its follow-up remains open.

See [Rust report](continuation-2026-10-24/provider-backup/validation.json),
[Android report](continuation-2026-10-24/provider-backup/android-validation.json),
[optimized JNI output](continuation-2026-10-24/provider-backup/jni-shrunk-smoke.log)
and [gate output](continuation-2026-10-24/provider-backup/backend-gate.json).
Raw logs are archived there and mirrored in latest/. The gate returns expected
exit 2 only for the eight open backend groups and requirement/inherited semantic
coverage; no stale input, mismatched report or failed-check reason remains.

Development checks are retained separately. The first twelve provider fixtures plus
thirteen controller fixtures passed. A later simulated-pipe assertion incorrectly
assumed descriptor sync must fail: Robolectric acknowledged the call even though
ciphertext verification passed. Raw failure output/XML is retained. The fixture now
explicitly injects unsupported sync and retains its assertion; all thirteen provider
fixtures and the complete final suite passed. This does not establish kernel-pipe
behaviour. No timeout/assertion is weakened and no private data is discarded.

## Remaining scope

The new tests use actual JNI and host-backed descriptors with synthetic providers;
the production OS-denied check is exercised, but successful real OS/Binder grants,
ART/kernel pipes, physical device performance, real cloud/provider durability and
final APK enforcement remain unproven. Metadata barriers still run serialized on the
writer; payload/provider I/O stays on the optional worker. No device SLO is invented.

Scheduled backup/persisted-grant consent, durable backup catalogue/status, JNI/platform
restore, orphan recovery/cleanup, bounded semantic restore validation, broader shared
resource scheduling and all other registered backend/coverage gaps remain open.
The existing core separately validates encrypted restore; it is not yet the Android
restore workflow. No SDK/model/licensing approval or production H.I. is fabricated.
Original ordered ink and explicit/manual proposal acceptance remain intact.

The handover folder remains ignored, untracked and absent from local path history;
local commit/push guards remain intact. No push was performed.
