package org.stemnotebook.backend

import org.junit.Assert.*
import org.junit.Test
import java.util.concurrent.CompletableFuture
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference

class SharedNotebookPoolTest {
    private class Resource {
        val ready = CompletableFuture<Void>()
        val closing = CompletableFuture<Void>()
        val released = CompletableFuture<Void>()
        val closed = CompletableFuture<Void>()
        val retireCalls = AtomicInteger()
        fun release() { closing.complete(null); released.complete(null); closed.complete(null) }
    }
    private fun pool(open: (String, Boolean) -> Resource) = SharedNotebookPool(open,
        { it.ready }, { it.closing }, { it.released }, { it.retireCalls.incrementAndGet(); it.closing.complete(null); it.closed })
    private fun unavailable(action: () -> Unit) { try { action(); fail("Unavailable runtime accepted") } catch (_: SharedNotebookPool.Unavailable) {} }
    private fun await(predicate: () -> Boolean) {
        val end = System.nanoTime() + TimeUnit.SECONDS.toNanos(10)
        while (!predicate()) { check(System.nanoTime() < end) { "Controlled fixture did not retire" }; Thread.sleep(1) }
    }
    @Test fun overlappingConsumersShareOneResourceAndOnlyLastLeaseRetiresIt() {
        val resource = Resource(); val opens = AtomicInteger(); val entered = CountDownLatch(1)
        val pool = pool { _, _ -> opens.incrementAndGet(); entered.countDown(); resource }
        val first = pool.acquire("one", true); assertTrue(entered.await(10, TimeUnit.SECONDS))
        val second = pool.acquire("one", false); resource.ready.complete(null)
        val a = first.await(); val b = second.await()
        assertSame(resource, a.value()); assertSame(resource, b.value()); assertEquals(1, opens.get())
        a.close(); a.awaitClosed(); assertEquals(0, resource.retireCalls.get())
        unavailable { a.value() }; assertSame(resource, b.value())
        b.close(); await { resource.retireCalls.get() == 1 }
        unavailable { pool.acquire("one", true) }
        resource.release(); b.awaitClosed(); pool.shutdownIdle()
    }
    @Test fun cancellationDoesNotCancelSharedOpenAndUnclaimedDeliveryCanBeAbandoned() {
        val resource = Resource(); val pool = pool { _, _ -> resource }
        val cancelled = pool.acquire("one", true); val survivor = pool.acquire("one", false)
        assertTrue(cancelled.cancel()); assertFalse(cancelled.cancel())
        try { cancelled.await(); fail() } catch (_: java.util.concurrent.CancellationException) {}
        resource.ready.complete(null); val lease = survivor.await()
        assertFalse(survivor.cancel()); assertEquals(0, resource.retireCalls.get())
        val unclaimed = pool.acquire("one", false); assertTrue(unclaimed.cancel())
        lease.close(); await { resource.retireCalls.get() == 1 }; resource.release(); lease.awaitClosed(); pool.shutdownIdle()
    }
    @Test fun cancelledOnlyOpenerRetainsSlotUntilActualResourceRetirement() {
        val resource = Resource(); val entered = CountDownLatch(1); val finishOpen = CountDownLatch(1)
        val pool = pool { _, _ -> entered.countDown(); check(finishOpen.await(10, TimeUnit.SECONDS)); resource }
        val pending = pool.acquire("one", false); assertTrue(entered.await(10, TimeUnit.SECONDS)); assertTrue(pending.cancel())
        unavailable { pool.acquire("one", true) }
        finishOpen.countDown(); resource.ready.complete(null); await { resource.retireCalls.get() == 1 }
        unavailable { pool.acquire("one", true) }; resource.release(); await { resource.closed.isDone }; pool.shutdownIdle()
    }
    @Test fun closeAcknowledgementWithoutOwnershipReleaseCannotAdmitAnotherOwner() {
        val first = Resource(); first.ready.complete(null); val second = Resource(); second.ready.complete(null)
        val opens = AtomicInteger(); val pool = pool { _, _ -> if (opens.incrementAndGet() == 1) first else second }
        val lease = pool.acquire("one", true).await(); lease.close(); await { first.retireCalls.get() == 1 }
        first.closed.complete(null); unavailable { pool.acquire("one", true) }
        val waiting = CountDownLatch(1); val done = CountDownLatch(1)
        val thread = Thread { waiting.countDown(); lease.awaitClosed(); done.countDown() }.apply { start() }
        assertTrue(waiting.await(10, TimeUnit.SECONDS)); assertFalse(done.await(30, TimeUnit.MILLISECONDS))
        first.released.complete(null); assertTrue(done.await(10, TimeUnit.SECONDS)); thread.join()
        val next = pool.acquire("one", false).await(); assertSame(second, next.value())
        next.close(); await { second.retireCalls.get() == 1 }; second.release(); next.awaitClosed(); pool.shutdownIdle()
    }
    @Test fun failedCloseIsReportedButDoesNotAssertOwnershipRelease() {
        val resource = Resource(); resource.ready.complete(null); val pool = pool { _, _ -> resource }
        val lease = pool.acquire("one", false).await(); lease.close(); await { resource.retireCalls.get() == 1 }
        resource.closed.completeExceptionally(IllegalStateException("private failure"))
        unavailable { lease.awaitClosed() }; unavailable { pool.acquire("one", false) }
        resource.released.complete(null); pool.shutdownIdle()
    }
    @Test fun nativeOpenFailureRetiresResourceBeforeAllowingReplacement() {
        val resource = Resource(); val entered = CountDownLatch(1); val pool = pool { _, create -> assertFalse(create); entered.countDown(); resource }
        val pending = pool.acquire("existing", false); assertTrue(entered.await(10, TimeUnit.SECONDS))
        resource.ready.completeExceptionally(IllegalStateException("private open failure")); unavailable { pending.await() }
        await { resource.retireCalls.get() == 1 }; unavailable { pool.acquire("existing", true) }
        resource.release(); pool.shutdownIdle()
    }
    @Test fun synchronousFactoryFailureDoesNotPublishResourceOrPoisonOtherLibrary() {
        val good = Resource(); good.ready.complete(null); val pool = pool { key, _ -> if (key == "bad") throw IllegalStateException("private") else good }
        val bad = pool.acquire("bad", false); unavailable { bad.await() }
        val lease = pool.acquire("good", true).await(); assertSame(good, lease.value())
        lease.close(); await { good.retireCalls.get() == 1 }; good.release(); lease.awaitClosed(); pool.shutdownIdle()
    }
    @Test fun externalCloseDeniesLeaseAccessAndOldLeaseCannotCloseReplacement() {
        val old = Resource(); old.ready.complete(null); val next = Resource(); next.ready.complete(null); val opens = AtomicInteger()
        val pool = pool { _, _ -> if (opens.incrementAndGet() == 1) old else next }
        val previous = pool.acquire("one", true).await(); old.closing.complete(null)
        unavailable { previous.value() }; unavailable { pool.acquire("one", true) }
        old.release(); val current = pool.acquire("one", false).await()
        previous.close(); previous.awaitClosed(); assertEquals(0, next.retireCalls.get()); assertSame(next, current.value())
        current.close(); await { next.retireCalls.get() == 1 }; next.release(); current.awaitClosed(); pool.shutdownIdle()
    }
    @Test fun boundedReservationsRejectExcessWithoutCancellingExistingConsumers() {
        val resource = Resource(); val pool = pool { _, _ -> resource }
        val pending = (1..128).map { pool.acquire("one", false) }
        unavailable { pool.acquire("one", false) }
        pending.drop(1).forEach { assertTrue(it.cancel()) }
        resource.ready.complete(null); val lease = pending.first().await(); assertSame(resource, lease.value())
        lease.close(); await { resource.retireCalls.get() == 1 }; resource.release(); lease.awaitClosed(); pool.shutdownIdle()
    }
    @Test fun interruptedObservationPreservesReservationForExplicitCloseOrLaterClaim() {
        val resource = Resource(); val pool = pool { _, _ -> resource }; val pending = pool.acquire("one", false)
        val observing = CountDownLatch(1); val interrupted = AtomicReference(false); val error = AtomicReference<Throwable?>()
        val thread = Thread {
            try { observing.countDown(); pending.await(); error.set(AssertionError("Interrupted observation claimed a lease")) }
            catch (_: java.util.concurrent.CancellationException) { interrupted.set(Thread.currentThread().isInterrupted) }
            catch (failure: Throwable) { error.set(failure) }
        }.apply { start() }
        assertTrue(observing.await(10, TimeUnit.SECONDS)); thread.interrupt(); thread.join(10000)
        assertFalse(thread.isAlive); assertNull(error.get()); assertTrue(interrupted.get()); assertEquals(0, resource.retireCalls.get())
        resource.ready.complete(null); val lease = pending.await(); assertSame(resource, lease.value())
        lease.close(); await { resource.retireCalls.get() == 1 }; resource.release(); lease.awaitClosed(); pool.shutdownIdle()
    }
    @Test fun retiringRootsKeepAdmissionSlotsUntilActualRelease() {
        val resources = (1..16).associate { "root$it" to Resource().apply { ready.complete(null) } }
        val opens = AtomicInteger(); val pool = pool { key, _ -> opens.incrementAndGet(); resources.getValue(key) }
        val leases = resources.keys.map { pool.acquire(it, false).await() }
        leases.forEach { it.close() }; await { resources.values.all { it.retireCalls.get() == 1 } }
        unavailable { pool.acquire("excess", false) }; assertEquals(16, opens.get())
        resources.values.forEach { it.release() }; leases.forEach { it.awaitClosed() }; pool.shutdownIdle()
    }
    @Test fun slowOpensCannotStarveAnotherRootsActualRetirement() {
        val existing = Resource().apply { ready.complete(null) }
        val slowA = Resource(); val slowB = Resource(); val entered = CountDownLatch(2)
        val pool = pool { key, _ -> if (key == "existing") existing else { entered.countDown(); if (key == "a") slowA else slowB } }
        val live = pool.acquire("existing", false).await()
        val a = pool.acquire("a", false); val b = pool.acquire("b", false)
        try {
            assertTrue(entered.await(10, TimeUnit.SECONDS))
            live.close(); await { existing.retireCalls.get() == 1 }
            existing.release(); live.awaitClosed()
            a.cancel(); b.cancel()
            await { slowA.retireCalls.get() == 1 && slowB.retireCalls.get() == 1 }
        } finally {
            a.cancel(); b.cancel(); live.close()
            slowA.ready.completeExceptionally(IllegalStateException("fixture stopped"))
            slowB.ready.completeExceptionally(IllegalStateException("fixture stopped"))
            slowA.release(); slowB.release(); existing.release()
        }
        pool.shutdownIdle()
    }
    @Test fun cancelledOpeningWaitsForActualOwnershipAndCloseFailureIsNotAReleaseWitness() {
        val resource = Resource(); val entered = CountDownLatch(1); val pool = pool { _, _ -> entered.countDown(); resource }
        val request = pool.acquire("one", false); assertTrue(entered.await(10, TimeUnit.SECONDS)); request.cancel()
        await { resource.retireCalls.get() == 1 }
        val done = CountDownLatch(1)
        val observer = Thread { request.awaitRetired(); done.countDown() }.apply { start() }
        resource.closed.completeExceptionally(IllegalStateException("close failed before release"))
        assertFalse(done.await(30, TimeUnit.MILLISECONDS)); unavailable { pool.acquire("one", false) }
        resource.ready.completeExceptionally(IllegalStateException("opening retired")); resource.released.complete(null)
        assertTrue(done.await(10, TimeUnit.SECONDS)); observer.join(); pool.shutdownIdle()
    }
    @Test fun nonLastCancelledReservationDoesNotWaitForAnotherConsumersOwnership() {
        val resource = Resource(); val pool = pool { _, _ -> resource }
        val first = pool.acquire("one", false); val second = pool.acquire("one", false)
        first.cancel(); first.awaitRetired(); assertEquals(0, resource.retireCalls.get())
        resource.ready.complete(null); val lease = second.await(); lease.close()
        await { resource.retireCalls.get() == 1 }; resource.release(); lease.awaitClosed(); lease.awaitRetired(); pool.shutdownIdle()
    }
    @Test fun callerCanAwaitCloseWithoutBlockingRegistryOrTheOpenWorker() {
        val resource = Resource(); resource.ready.complete(null); val pool = pool { _, _ -> resource }
        val done = CountDownLatch(1); val error = AtomicReference<Throwable?>()
        val observer = Thread {
            try { val lease = pool.acquire("one", true).await(); lease.close(); lease.awaitClosed() }
            catch (failure: Throwable) { error.set(failure) }
            finally { done.countDown() }
        }.apply { start() }
        await { resource.retireCalls.get() == 1 }; resource.release()
        assertTrue(done.await(10, TimeUnit.SECONDS)); observer.join(); assertNull(error.get()); pool.shutdownIdle()
    }
}
