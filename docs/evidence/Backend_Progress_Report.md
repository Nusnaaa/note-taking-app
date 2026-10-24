# Phase A progress — authoring, Android boundary and backend testing

> Current continuation status — 24 October 2026: backend acceptance remains NOT MET
> and production H.I. is not authorised. The latest validated source is `c4df7ab`;
> see [provider backup evidence](Provider_Backup_2026-10-24.md) and
> [continuation provenance](Backend_Continuation_2026-10-23.md). Its 294 Rust
> tests, 167 Android tests, seventeen JNI groups before/after R8 and matching
> exact-source validation passed.
> Prior E01 setup-timeout evidence remains retained with an open follow-up.
> The 22 October report below is historical; its original `latest/` artifacts are
> recoverable at `83b80b7`. The current `latest/` directory records the newer run.

22 October 2026. **The requested final 40% is not fully complete. Backend acceptance
remains NOT MET; production H.I. remains 0%.** This is a substantial tested implementation
increment, not a completed Android notebook. The previous approximate 60% checkpoint
was not a measured requirement-completion score, and this report does not invent a new
percentage. Remaining implementation is listed below, not hidden as manual testing.

Validated source: `653c151bb6a755294a4215853b6a61d5d29f30a0`, branch `phase-a/backend`. Rust and Android
validation use the same source SHA-256 manifest with no uncommitted source changes.
Evidence outputs are excluded from their own input fingerprint; the full pre-run Git
status is retained separately. This report/logs are committed afterwards.

## Implemented since the 108-test checkpoint

| Area | Delivered behaviour |
| --- | --- |
| Durable authoring undo/redo | Transactional blocks, layouts, groups and embeds; revision advancement, atomic selection edits, reopen/backup recovery, current-lock enforcement, protected retention and purged-payload retirement |
| Region recognition | Explicit selected-stroke partition/split/merge, immutable original ownership and revision/fingerprint-safe per-region proposal acceptance; accepted region text/maths/code participates in search/export |
| Host protocol | Bounded single-writer JSON v1 boundary, trusted authentication callback, authoring/exercise/navigation/object/view/link operations, content-free failures and a developer CLI |
| JNI/Android library | Opaque handles and panic boundary; actual ARM64 build; Kotlin asynchronous adapter and platform BiometricPrompt/device-credential verifier; library backup/transfer exclusions and INTERNET removal directive |
| Observed capture/recovery | MotionEvent history/current samples, pointer cancellation, null unsupported channels, exact vector encoding, bounded durable journal, immutable tickets, ordered replay, restart recovery, low-space reserve release and draining close |
| Processing lifecycle | Consented local resumable resources, interrupted-tail recovery, hash-checked publication, resource leases and durable converter activation/failed-update rollback |
| PDF components | Read-only content-provider adapter and isolated parser/rendering service, plus paginated vector-maths/selectable-Unicode PDF export with supplied local fonts |
| Portability | Native subset v4 regions with v1–v3 reads; schema-9 encrypted whole backup including authoring undo; schema-8 restoration with empty new undo logs; historical-row validation |
| Navigation | Flat visible exercises and permission-filtered search/open-in-master projections without duplicating content |

The Android module has no Activity, Compose screen or production H.I. Its AAR is a backend
library, not an installable notebook. The dev CLI can be used now with synthetic data;
`docs/verification/HOST_INTEGRATION.md` contains prerequisites, commands and protocol usage.
The CLI has no authentication bypass. Owner-private pen/model/manual tests stay deferred
until the complete runnable app, as previously authorised.

## Executed validation

| Check | Result |
| --- | --- |
| Rust format and strict Clippy, all targets | PASS |
| Full Rust workspace suite | **134 passed; 0 failed; 0 ignored** |
| Desktop release build | PASS |
| ARM64 Android native compilation and ELF identity | PASS |
| Actual desktop JVM/JNI smoke checks, `-Xcheck:jni` | PASS, 6 recorded check groups |
| Android unit/Robolectric API-35 suite | **10 passed; 0 failures/errors/skips** |
| Android library compilation and lint | PASS |
| AAR native payload | PASS, ARM64 library present (3,956,440 bytes) |
| Library manifest policy | PASS: retained removal directive, backup/transfer exclusions, no cleartext, isolated non-exported parser |
| CLI streamed commands and cross-process reopen | PASS |
| Requirement ledger consistency | PASS: 94 requirements, 103 inherited clauses, 158 candidates retained |
| Complete backend acceptance | **NOT MET**, gate exit 2 |

Rust validation: 2026-10-22T11:08:59.868282+00:00 to 2026-10-22T11:09:41.292248+00:00.
Android validation: 2026-10-22T11:08:50.079511+00:00 to 2026-10-22T11:09:26.784684+00:00.
Exact commands, source hashes, output and results are in `latest/validation.json`,
`latest/android-validation.json` and adjacent logs. The successful Android runner also
records the native/AAR SHA-256 values. Test counts describe executed component checks,
not complete requirement acceptance. The preceding checkpoint was 108 Rust tests.

The 10 Android tests comprise four accumulator tests, two MotionEvent tests and four
outbox/encoding/recovery tests. Robolectric simulates framework behaviour; it is not a
physical tablet or emulator run. The JNI fixture uses the actual native library and
trusted test callback, not real biometrics. No device latency, hardware power-loss,
recogniser accuracy, runtime egress audit or independent security audit is claimed.

The synthetic renderer fixture produced a three-page A4 PDF with selectable text and
vector maths. Its metadata/text were inspected with Poppler and its rendered first page
was visually checked. Unsupported glyphs and unsupported image/embed fidelity fail
explicitly; this fixture is not whole-export acceptance.

## Findings and corrections

Tests/review corrected strict JSON handling for fieldless commands, revision-safe host
operations, transient build issues, active-use history pruning across protected blocks,
permanent-deletion undo-payload retirement and malformed backup journal identities.
The MotionEvent fixture was corrected to compare actual framework-observed values;
the independent accumulator/encoder tests still preserve a measured negative zero.

The first final Android run passed compilation, JNI, all Android tests and lint, then
failed an overly broad validation assertion: library manifests retain `tools:node=remove`
markers. The validator now checks that directive as the AAR merge contract and separately
records that final APK/device enforcement remains untested. The failed attempt and reason
are retained in `latest/android-first-attempt.json` and `.log`. The corrected complete
run above passed; no failed/ignored test was reclassified as passing.

## Still incomplete

| Area | Remaining implementation/acceptance |
| --- | --- |
| Recognition runtime | An eligible actual local recogniser, complete model-resource lease integration and hard worker termination/scheduling |
| Rich documents/canvases | Flow/reflow, layers, embed overlays/follow-object, component-cell recognition and complete image/canvas/diagram export fidelity |
| PDF host | Client binding, provider/revocation/timeout orchestration, policy synchronisation and full annotation/navigation workflows |
| Portability/recovery | Chunked/scalable backup beyond the current 64 MiB in-memory limit, complete lineage/PDF-linked native portability and all-operation history |
| Interaction state/capture | Caret/selection state, live subscriptions, active-stroke interruption policy and full host priority/emergency orchestration |
| Android acceptance | Remaining workflow boundaries, final app lifecycle/manifest, device authentication/privacy/process integration and interrupt/resource cases |
| Full verification | Closure of every applicable canonical requirement, inherited clause and retained candidate assertion |

These are substantive implementation gaps. All eight gap groups remain OPEN in
`docs/verification/backend-gaps.json`; the gate additionally reports incomplete canonical
and inherited-clause coverage. It reports no stale source, missing automated check,
uncommitted source or mismatched desktop/Android source hashes. Passing this subset
does not authorise production H.I. or claim completion of the owner's requested milestone.

## Source and reproducibility

Actual unpacked source/tests are committed in Git; no ZIP replaces source history.
New main commits: `1c2bf21` (core/JNI/rendering), `b4e98e5` (capture recovery/host workflows),
`653c151` (AAR validation correction). An evidence-only commit follows. The Git bundle
preserves the full branch history alongside individually readable files. No remote has
been configured or pushed. No proprietary SDK/model weights, private handwriting,
credentials, font binaries or build-tool downloads are committed.
