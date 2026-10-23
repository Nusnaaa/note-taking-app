package org.stemnotebook.backend

import org.junit.Assert.*
import org.junit.Test
import java.util.Collections
import java.util.concurrent.CountDownLatch
import java.util.concurrent.ExecutionException
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.TimeUnit

class BackendWriterTest {
    @Test(timeout = 10_000) fun reservedCountsIncludeRunningWorkAndQueuedReadsYieldToFifoEdits() {
        val writer = BackendWriter(BackendWriter.Limits(2, 100, 2, 100))
        val entered = CountDownLatch(1); val release = CountDownLatch(1); val order = Collections.synchronizedList(mutableListOf<String>())
        try {
            val running = writer.admit(false, 10) { entered.countDown(); release.await(); order += "running-read" }
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            val read = writer.admit(false, 20) { order += "queued-read" }
            assertThrows(RejectedExecutionException::class.java) { writer.admit(false, 1) {} }
            val edit = writer.admit(true, 30) { order += "capture" }
            val access = writer.admit(true, 40) { order += "relock" }
            assertThrows(RejectedExecutionException::class.java) { writer.admit(true, 1) {} }
            assertEquals(2, writer.snapshot().pendingAuthoring); assertEquals(70L, writer.snapshot().authoringBytes)
            assertEquals(2, writer.snapshot().pendingQueries); assertEquals(30L, writer.snapshot().queryBytes)
            release.countDown(); listOf(running, read, edit, access).forEach { it.get(5, TimeUnit.SECONDS) }
            assertEquals(listOf("running-read", "capture", "relock", "queued-read"), order)
            assertEquals(0L, writer.snapshot().authoringBytes); assertEquals(0L, writer.snapshot().queryBytes)
        } finally { release.countDown(); writer.shutdown(); assertTrue(writer.awaitTermination(5, TimeUnit.SECONDS)) }
    }
    @Test(timeout = 10_000) fun byteBudgetsCannotBorrowAcrossLanesAndRecoverOnlyAfterActualCompletion() {
        val writer = BackendWriter(BackendWriter.Limits(5, 10, 5, 8)); val entered = CountDownLatch(1); val release = CountDownLatch(1)
        try {
            val running = writer.admit(true, 10) { entered.countDown(); release.await() }; assertTrue(entered.await(5, TimeUnit.SECONDS))
            assertThrows(RejectedExecutionException::class.java) { writer.admit(true, 1) {} }
            val query = writer.admit(false, 8) { 7 }
            assertThrows(RejectedExecutionException::class.java) { writer.admit(false, 1) {} }
            assertEquals(1, writer.snapshot().pendingAuthoring); assertEquals(1, writer.snapshot().pendingQueries)
            release.countDown(); running.get(5, TimeUnit.SECONDS); assertEquals(7, query.get(5, TimeUnit.SECONDS))
            assertEquals(9, writer.admit(true, 10) { 9 }.get(5, TimeUnit.SECONDS))
            assertThrows(RejectedExecutionException::class.java) { writer.admit(true, 11) {} }
        } finally { release.countDown(); writer.shutdown(); assertTrue(writer.awaitTermination(5, TimeUnit.SECONDS)) }
    }
    @Test(timeout = 10_000) fun cancelledObservationAndShutdownCannotRetractAdmittedInkOrInterruptRunningWork() {
        val writer = BackendWriter(); val entered = CountDownLatch(1); val release = CountDownLatch(1); val committed = CountDownLatch(1)
        try {
            val running = writer.admit(true, 10) { entered.countDown(); release.await() }; assertTrue(entered.await(5, TimeUnit.SECONDS))
            val ink = writer.admit(true, 20) { committed.countDown() }; assertTrue(ink.cancel(true))
            assertTrue(writer.shutdownNow().isEmpty()); assertTrue(writer.isShutdown); assertFalse(writer.isTerminated)
            assertFalse(writer.awaitTermination(1, TimeUnit.MILLISECONDS))
            assertThrows(RejectedExecutionException::class.java) { writer.admit(true, 1) {} }
            assertEquals(2, writer.snapshot().pendingAuthoring)
            release.countDown(); running.get(5, TimeUnit.SECONDS); assertTrue(committed.await(5, TimeUnit.SECONDS))
            assertTrue(writer.awaitTermination(5, TimeUnit.SECONDS)); assertEquals(0, writer.snapshot().pendingAuthoring)
        } finally { release.countDown(); writer.shutdown(); assertTrue(writer.awaitTermination(5, TimeUnit.SECONDS)) }
    }
    @Test(timeout = 10_000) fun subscriptionsCoalesceAndWakeWithoutObserverCallbacksOnWriter() {
        val writer = BackendWriter(); val slow = writer.subscribe(); val fast = writer.subscribe()
        try {
            val initial = checkNotNull(fast.next()); assertEquals(initial, slow.next()); assertNull(fast.next())
            writer.admit(false, 1) { 1 }.get(5, TimeUnit.SECONDS)
            writer.admit(true, 2) { 2 }.get(5, TimeUnit.SECONDS)
            val latest = checkNotNull(slow.next()); assertEquals(0, latest.pendingAuthoring); assertTrue(latest.sequence > initial.sequence + 1)
            assertEquals(latest, fast.next()); assertNull(slow.next(1))
            val observed = java.util.concurrent.CompletableFuture.supplyAsync { slow.next(5_000) }
            writer.markAttention(); assertTrue(checkNotNull(observed.get(5, TimeUnit.SECONDS)).needsAttention)
            writer.shutdown(); assertTrue(writer.awaitTermination(5, TimeUnit.SECONDS)); assertTrue(checkNotNull(slow.next()).closed)
            assertNull(slow.next(5_000)) // Closed subscriptions never wait for another event.
        } finally { writer.shutdown(); assertTrue(writer.awaitTermination(5, TimeUnit.SECONDS)) }
    }
    @Test(timeout = 10_000) fun actionFailuresRetireCapacityAndAuthoringAttentionIsSticky() {
        val writer = BackendWriter()
        try {
            assertThrows(ExecutionException::class.java) { writer.admit(false, 3) { error("synthetic read failure") }.get(5, TimeUnit.SECONDS) }
            assertFalse(writer.snapshot().needsAttention)
            assertThrows(ExecutionException::class.java) { writer.admit(true, 5) { error("synthetic edit failure") }.get(5, TimeUnit.SECONDS) }
            assertTrue(writer.snapshot().needsAttention)
            assertEquals(11, writer.admit(true, 1) { 11 }.get(5, TimeUnit.SECONDS)); assertTrue(writer.snapshot().needsAttention)
            assertEquals(0, writer.snapshot().pendingAuthoring); assertEquals(0, writer.snapshot().pendingQueries)
        } finally { writer.shutdown(); assertTrue(writer.awaitTermination(5, TimeUnit.SECONDS)) }
    }
    @Test fun unusedWriterClosesWithoutStartingAThread() {
        val writer = BackendWriter(); writer.shutdown(); writer.shutdown()
        assertTrue(writer.isTerminated); assertTrue(writer.awaitTermination(0, TimeUnit.SECONDS))
        assertTrue(checkNotNull(writer.subscribe().next()).terminated)
        assertThrows(RejectedExecutionException::class.java) { writer.execute {} }
    }
}
