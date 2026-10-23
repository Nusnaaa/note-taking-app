# Library ownership and complete resource retirement — 23 October 2026

Final source checkpoint: `32ad19e`, branch `phase-a/backend` (ownership/drain
`21f433b`, SQLite-managed native ownership `fa56293`, and the canonical-root
pre-reservation correction `32ad19e`).

Backend acceptance remains NOT MET and production H.I. remains gated. This closes
specific ownership/shutdown paths, not all A1/A2 lifecycle or semantic coverage.

Final exact-source validation passed with matching clean source manifests:
**164 Rust tests**, **70 Android unit/Robolectric tests**, and **nine desktop JNI
smoke groups**, with no failed/ignored/skipped tests. Format, strict workspace
Clippy, release build, traceability, ARM64 compilation/ELF, Android lint, AAR native
payload and isolated-service manifest checks passed. Exact commands, hashes and
logs are in `continuation-2026-10-23/library-ownership/final/` and `latest/`.

## Final implemented behavior

Native Host claims SQLite exclusive locking and an initial transaction before
migrations or authoring. SQLite retains its connection lease across commits and
manages all database descriptors/record locks. Duplicate Host/JNI opens and pathname
aliases are rejected without truncating data; independent libraries remain usable.
The native writer closes SQLite only after draining all admitted work. No extra
raw database FD is opened as an ownership guard.

`Host::close` closes admission and waits for its writer despite surviving Arc
callers. Concurrent closes share a join and repeated closes are safe. Writer-thread
identity is checked before the join mutex, so a verifier cannot self-join or deadlock
behind an external closer waiting on it. Such reentrant close fails closed. JNI
removes the handle under its registry lock, releases that lock and waits for actual
Host drain. A blocked close leaves another library's registry/workflows usable.

Android reserves the canonical root in process, then acquires a separate private
OS file lease before constructing factories or
retiring crash files. Duplicate/canonical-root aliases fail before cleanup; symlinked
notebook directories are rejected. A duplicate is rejected before opening another FD, so closing its rejected channel
cannot drop the process's POSIX lease. The owner file is kept to avoid an unlink/inode
race. No native handle opens until synchronous factories succeed; constructor and
asynchronous native-open failures close their resources and retire ownership.

`closeAsync` acknowledges native writer drain. `closeFullyAsync` also awaits actual
provider opening/cancellation, parser bind/pixel/disposal/control threads and source/
selection/cache workers before releasing app ownership. An uncooperative platform
operation keeps full shutdown pending and blocks a replacement backend; it does not
permit late publication or another owner deleting an active cache. The closer runs
away from main/writer/control threads. Original ink and capture journals retain
their existing commit/recovery contracts.

## Executed scope and development findings

Native fixtures exercise duplicate and hard/symbolic aliases, failed initialization,
independent libraries, actual CLI subprocess exclusion, abrupt child exit and exact
original-ink recovery, queued capture through close with live Arc callers, and
reentrant close while an external close is waiting. A unit fixture verifies SQLite
exclusive mode across commits and rejected/closed duplicate connections. Standalone
actual JNI adds concurrent close/drain and independent registry access.

Actual desktop JNI Android fixtures reject duplicate/root/library-directory aliases
before factories/cleanup, use an independent JVM to verify the OS lease remains
held after rejected duplicates, distinguish native drain from blocked provider and parser
retirement, verify late descriptor closure, clean up failed factories/workspace/native
opens and reopen after full retirement. Automatic native-open failure retirement
is checked before calling explicit close. Existing PDF integration fixtures now await
full retirement before disposing their temporary roots.

An exploratory Kotlin compile found a retirement helper accidentally placed inside
the nested Review class; it was moved to the manager. Its 70-method Android run then
passed. One parallel native drain fixture failed under the initial raw-FD lock,
while isolated and traced reruns passed. An inherited-descriptor test established
that closing only a parent descriptor cannot reliably retire that raw-FD lock.
More significantly, reviewing the pinned bundled SQLite unixVFS source confirmed
that closing an unrelated raw FD can release that process's POSIX SQLite locks.
The raw-FD approach was therefore superseded even though its complete 164-Rust/
70-Android/nine-JNI run passed. That earlier exact-source run remains under
`library-ownership/first-attempt/`; only the final SQLite-managed run establishes
the current checkpoint. A separate JDK probe also observed that closing a rejected
duplicate lease channel could leave the process's OS lease available to another JVM,
despite Java's in-process FileLock still being valid. A canonical-root reservation
was added before any lease FD opens, and an independent-JVM regression now verifies
OWNED after rejected duplicates and AVAILABLE only after full retirement. The
SQLite-managed first pass at `fa56293` is retained under `sqlite-first-pass/`;
final evidence uses the subsequent corrected source. No failed or superseded result
closes requirements.

These checks establish desktop cooperative SQLite/Java file locking and simulated
Android resource lifetimes. They do not establish actual device Binder/process
termination, physical interruption performance or final APK enforcement. Independent
Android admission/priority/status, active-stroke interruption/recovery and the other
backend groups remain open. No production H.I., model weights, handwriting upload,
remote inference, automatic OS content backup or origin push is introduced.
`handover-do-not-commit/` remains ignored, untracked and outside branch history.

The final backend gate exits 2 for substantive open groups and incomplete canonical/
inherited semantic coverage only. No stale/mismatched/incomplete automated-evidence
reason remains.
