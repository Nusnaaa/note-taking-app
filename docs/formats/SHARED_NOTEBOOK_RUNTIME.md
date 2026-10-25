# Shared notebook runtime

`NotebookRuntime` offers process-local leases to one live `NotebookBackend` per
canonical no-backup root. Foreground work and the future scheduled-backup service
must use this same entry point to share a live backend. Existing direct backend
constructors stay exclusive; the runtime never adopts or displaces their owner.
The [optional scheduling service](SCHEDULED_BACKUPS.md) now uses this entry point;
runtime acquisition alone enables no schedule and adds no production H.I.

Acquire, await and awaitClosed run on a worker. The default existing-only open
cannot create an empty notebook; the deliberate foreground first-use workflow
may request creation. Joining an already live foreground backend does not itself
create another database. Background policy must still verify its expected native
library UUID, current access, capture, recovery and source state before running.
Runtime possession is not protected-content authority.

Worker-side use (the consumer must first retire its own admitted operations):

```kotlin
val lease = NotebookRuntime.acquire(context, createIfMissing = false).use { it.await() }
try {
    val backend = lease.backend
    // Await this consumer's admitted operations and cleanup here.
} finally {
    lease.close()
    lease.awaitClosed()
}
```

Each acquisition reserves a distinct pending request. Waiting/cancellation uses
private signals, so caller future continuations cannot run on the native writer,
registry monitor or open/close worker. Await transfers the reservation into one
lease exactly once; cancel before transfer releases only that reservation. An
available but unclaimed request can still be abandoned. An interrupted observation
preserves the thread interrupt and leaves the reservation for explicit close;
it does not cancel another consumer or the shared open.

Callers close a lease only after their own operations have actually retired.
Closing a non-last lease keeps the backend open for other consumers. The last
lease requests complete backend retirement; awaitClosed waits for the resource
close result and successful ownership release. A close failure is reported, but
is not treated as proof that ownership was released. A dedicated internal signal
is completed only after the existing LibraryOwnership.close succeeds. Earlier
cleanup failures leave that signal pending and deny a replacement owner.

Slots survive Opening/Retiring until actual release. New acquisitions fail during
retirement. Canceling the last opening request cannot permit a second owner while
the first factory/resource is still alive. External backend close denies new
leases and closes ordinary backend admission; an old lease cannot retire a newer
replacement slot. No registry monitor is held during factories, platform/native
calls, close, or future signal completion. Synchronous factory failure is reported;
existing LibraryOwnership still prevents reopening before failed-constructor
cleanup actually releases its own lease.

Separate bounded pools of two open and two retirement workers, at most 16 active/opening/retiring roots and 128 lease
reservations bound this registry's control work. These are defensive admission
caps, not a device performance or latency promise. A retiring root keeps its
slot, so cancellation cannot turn ongoing resource work into unlimited new opens.
Caller code must not retain/use a backend after releasing its lease. The shared
runtime does not override the backend's capture-first optional-work policy.

Host lifecycle tests use controlled resources for overlap, cancellation, slow opens without retirement starvation, late
ownership release, failed close/open, external close/replacement and bounded
admission. JNI/Robolectric integration covers a real shared backend, last-owner
release/reopen, canonical aliases, existing-only missing-library rejection,
exclusive-owner preservation and main-thread refusal. JobService stop is additionally covered by host callback fixtures, while real
OS/device/provider/Keystore and physical interruptions remain unexecuted. The
public scheduling controller/service is now integrated for the primary root; this
component does not close the full backend gap by itself.

The OS service also needs actual reservation retirement after canceling an open.
Pending.awaitRetired waits the ownership witness when its abandoned reservation
was last; it does not wait for unrelated consumers. Lease.awaitRetired similarly
waits actual release even if awaitClosed reports an earlier failure. Runtime Owner
wrappers preserve the success-only witness captured immediately on ownership
acquisition, including synchronous constructor failure. A canceled observer alone
is never proof that native opening/provider cleanup retired.
