# Durable active-capture recovery — 23 October 2026

Source checkpoint: `6a824bb`, branch `phase-a/backend`; implementation `7ef4b51`.
Backend acceptance remains NOT MET and production H.I. remains gated.

Full exact-source validation passed with matching clean source manifests:
**172 Rust tests**, **90 Android unit/Robolectric tests**, and **ten desktop JNI
smoke groups**, with no failed/ignored/skipped tests in the final run. Format,
strict workspace Clippy, release build, traceability, ARM64 compilation/ELF,
Android lint, AAR payload and isolated-service manifest checks passed. Commands,
hashes and logs are in `continuation-2026-10-23/active-capture-journal/` and `latest/`.

## Delivered behavior

Backend-owned MotionEvent sessions retain stable stroke identities and exact
observed prefixes in a bounded library-bound private journal. Binary records keep
raw floating-point bits, optional-channel absence, sample order and explicit
termination provenance. Revision checks reject stale, altered or rebound updates.
UTF encoding rejects malformed coordinate spaces rather than silently replacing
source data. File synchronization, atomic replacement and directory synchronization
precede the durable recovery acknowledgement. Record/count quotas never evict ink.

An active checkpoint acknowledges recovery storage, not SQLite document commit.
Completed or explicitly lifecycle-interrupted records use existing exact-ticket
native capture receipts. Ambiguous acknowledgements retry without duplicate ink.
Cancellation retires tentative contacts and rejects stale resurrection. Caller-owned
observations remain available after queue/storage rejection. Cancelling a returned
observation future no longer skips an already admitted journal or legacy capture.

Startup reconciles valid interrupted temporary writes, preserves torn evidence,
and checks checksums and library identity. Surviving active prefixes become explicit
Interrupted(Recovery) evidence requiring fresh scoped review and confirmation before
publication. Deliberate discard also checks current access and the review checksum.
Unreviewed or protected recovery blocks its own session, while independent sessions
can continue. Graceful close seals each session independently without synthesizing a
pen-up point; a failed checkpoint is reported after other sessions and resources
have retired. The opaque native library-identity getter works while locked but grants
no notebook content access. CaptureReceipt remains subject to current entity/block
protection and ticket binding.

## Executed fixture scope and first attempt

Component tests cover exact prefix extension, quota admission, stale/rebound writes,
cancellation, failed native delivery, ambiguous acknowledgements, foreign libraries,
corrupt authoritative files and torn temporary preservation. Independent child JVMs
halt at file synchronization, rename and directory synchronization, bypassing normal
cleanup; reopening retains exact observed samples as reviewable recovery evidence.
These are actual JVM termination fixtures, not physical storage/power-loss tests.

Actual desktop-JNI/Robolectric integration covers MotionEvent checkpoint versus
commit, exact retries, cancellation/reopen, graceful interruption, startup review,
fresh protection checks, independent input, explicit discard, cancelled observation
futures, and independent close sealing after one session fails. Native fixtures
cover scoped receipt probes and locked-library identity without content access.
Synthetic MotionEvents establish no physical tablet delivery or latency guarantee.

The first full Android attempt failed an existing PDF late-binding fixture: its
factory-created latch did not establish that connect() had installed the callback.
Cancellation could therefore precede installation, and the test dereferenced null.
The fixture now publishes a connection-installed latch and the late-callback test
waits for that prerequisite. The original failed Android report/log and the first
successful Rust run are preserved in `first-attempt/`; the final run validates the
corrected source without suppressing or skipping that test.

Still open: Android count/byte admission, capture priority and status observation;
automated device interruption/resource coverage; other rich-object, lifecycle,
portability and complete semantic acceptance work in the gap register. No production
H.I., weights, handwriting upload, remote inference, automatic OS content backup or
origin push is introduced. The handover folder remains ignored, untracked and absent
from reachable repository history; local commit/push guards remain installed.

The final backend gate exits 2 for substantive open implementation groups and
incomplete canonical/inherited coverage only, with no stale or incomplete automated
evidence reason.
