# Capture priority over recognition — 24 October 2026

Source checkpoint: `d87531d`, branch `phase-a/backend`.
Backend acceptance remains NOT MET; production H.I. remains gated.

## Delivered behavior

Trusted observed-contact signals now cancel the native recognition epoch and reject
new source admission while any contact is active. End/begin are idempotent and scoped
by session; one session cannot release another. Ending contact permits new jobs only,
never reviving cancelled work. Cooperative adapters observe cancellation and deliberately
late success is rejected. Running reservations/library ownership remain pinned until
actual retirement. Idle input/runtime disposal stays on the optional cleanup lane.

The JNI signal carries no handwriting, document identity, output or readiness claim.
It neither waits for SQLite nor invokes an SDK callback. Native session admission is
bounded to 16; overflow disables optional recognition for that host lifetime while
preserving ink authoring. This is an engineering admission bound, not an approved SLO
or proof of CPU scheduling/hard runtime termination.

Android observed pen/opted-in finger sessions hold priority through the actual final
checkpoint attempt. Frame generations prevent delayed pen-up acknowledgement from
releasing a newer contact. Private admission completion is independent of caller receipt
cancellation. Lifecycle interruption, overlapping sessions and pre-open current-state
reconciliation retain their boundaries. Unexpected signal errors are exposed in
content-free status without discarding observations or replacing save receipts.

## Development findings and coverage

Three native fixtures exercise nested/idempotent sessions, current prepared/computed/
running work cancellation, late success rejection, independent source admission,
authoring while an adapter is deliberately blocked, and fail-closed optional overflow.
Three Android JNI fixtures exercise overlapping sessions/interruption, old pen-up
completion during a newer contact and cancellation of a receipt while the real write
is blocked. Existing tests remain in scope.

The initial Android fixture used empty authentication scopes, which correctly rejected
authentication without invoking its blocking verifier. That test setup was corrected to
the established App scope; its failed log/XML are retained under development/. A new
isolated regression then genuinely failed because cancelling receipt observation caused
priority to retire before the admitted pen-up checkpoint ran. The fix uses private
admission completion and returns a separate dependent observation future. Its original
failure is retained separately. Neither development attempt is an acceptance run or
attributed to a final frozen source manifest. No timeout/assertion was weakened.

A complete clean-source run at 814d0e6 passed 261 Rust and 135 Android tests and all
validation stages, but exposed a Kotlin generated-copy visibility compatibility warning
on the newly non-public Submission constructor. That constructor was restored to public
visibility (as in the preceding API); the backend completion remains private and receipt
observation remains a separate dependent future. The full prior run is preserved under
constructor-warning/. Final corrected-source validation passed at d87531d with clean matching source
manifests: **261 Rust tests**, **135 Android tests**, ten JNI smoke groups, format,
strict Clippy, release, traceability, ARM64/ELF, Android lint, AAR and merged-manifest
checks. No tests failed, were skipped or ignored. The constructor warning is absent.
Commands, timings, hashes and raw output are archived at the capture-priority root
and latest/. E01 did not recur; its separate cause remains unestablished.

## Remaining work

Actual Android recogniser-provider registration/execution, an eligible approved runtime,
complete cross-media resource scheduling and hard worker isolation remain open. This
change does not claim model quality, device pen latency, zero-loss behavior, SDK rights
or complete backend acceptance. E01 remains a separate historical open follow-up.

The handover folder remains ignored/untracked and excluded from local commits. No push
was performed.
