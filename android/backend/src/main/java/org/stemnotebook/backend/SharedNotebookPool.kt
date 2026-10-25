package org.stemnotebook.backend

import java.io.Closeable
import java.io.IOException
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.CompletableFuture
import java.util.concurrent.CompletionException
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit

/** Shared lifetime, with no exposed future continuations on the open/close worker.
 * Limits bound control admission, not device throughput or responsiveness. */
internal class SharedNotebookPool<K : Any, T : Any>(
    private val open: (K, Boolean) -> T,
    private val ready: (T) -> CompletableFuture<Void>,
    private val closing: (T) -> CompletableFuture<Void>,
    private val released: (T) -> CompletableFuture<Void>,
    private val retire: (T) -> CompletableFuture<Void>,
) {
    class Unavailable : IOException("Shared notebook runtime unavailable")
    enum class State { Opening, Open, Retiring, Retired }
    enum class TicketState { Waiting, Available, Claimed, Cancelled, Failed }
    class Entry<K, T>(val key: K, val create: Boolean) {
        var state = State.Opening
        var resource: T? = null
        var users = 0
        val waiters = mutableSetOf<Ticket<K, T>>()
        var closeStarted = false
        var closeSucceeded = false
        var ownershipReleased = false
        val closeAck = CompletableFuture<Void>()
    }
    class Ticket<K, T>(val entry: Entry<K, T>) {
        var state = TicketState.Waiting
        val signal = CompletableFuture<Void>()
    }
    private val lock = Any()
    private val entries = mutableMapOf<K, Entry<K, T>>()
    private var users = 0
    // At most 16 entries, each with one open and one close control task. Closing
    // keeps its slot until actual release. Cancelled requests cannot evade caps.
    private fun controls(name: String) = ThreadPoolExecutor(2, 2, 0, TimeUnit.MILLISECONDS, ArrayBlockingQueue<Runnable>(64),
        { action -> Thread(action, name).apply { isDaemon = true } }, ThreadPoolExecutor.AbortPolicy())
    private val opener = controls("stem-shared-open")
    // Slow opening must never occupy the workers needed to retire another root.
    private val closer = controls("stem-shared-retire")

    inner class Pending internal constructor(private val ticket: Ticket<K, T>) : Closeable {
        /** Blocking worker operation; observation interruption does not cancel
         * another lease or a shared open. Call close to abandon this reservation. */
        fun await(): Lease {
            try { ticket.signal.get() }
            catch (_: InterruptedException) {
                Thread.currentThread().interrupt()
                throw java.util.concurrent.CancellationException("Notebook runtime observation interrupted")
            } catch (_: java.util.concurrent.ExecutionException) { throw Unavailable() }
            var failure = false
            var close: Entry<K, T>? = null
            val lease = synchronized(lock) {
                if (ticket.state == TicketState.Cancelled) throw java.util.concurrent.CancellationException("Notebook runtime request cancelled")
                check(ticket.state == TicketState.Available) { "Notebook runtime request already consumed" }
                if (ticket.entry.state != State.Open) {
                    ticket.state = TicketState.Failed
                    close = releaseLocked(ticket.entry)
                    failure = true
                    null
                } else {
                    ticket.state = TicketState.Claimed
                    Lease(ticket.entry, checkNotNull(ticket.entry.resource))
                }
            }
            close?.let { enqueueClose(it) }
            if (failure) throw Unavailable()
            return checkNotNull(lease)
        }
        fun cancel(): Boolean {
            var close: Entry<K, T>? = null
            val applied = synchronized(lock) {
                if (ticket.state != TicketState.Waiting && ticket.state != TicketState.Available) false else {
                    ticket.entry.waiters.remove(ticket)
                    ticket.state = TicketState.Cancelled
                    close = releaseLocked(ticket.entry)
                    true
                }
            }
            if (applied) ticket.signal.completeExceptionally(java.util.concurrent.CancellationException("Notebook runtime request cancelled"))
            close?.let { enqueueClose(it) }
            return applied
        }
        override fun close() { cancel() }
    }
    inner class Lease internal constructor(private val entry: Entry<K, T>, private val resource: T) : Closeable {
        private var closed = false
        private var last = false
        /** The caller must not retain/use the backend after releasing its lease. */
        fun value(): T = synchronized(lock) { if (closed || entry.state != State.Open) throw Unavailable(); resource }
        override fun close() {
            val close = synchronized(lock) {
                if (closed) return
                closed = true
                val action = releaseLocked(entry)
                last = entry.users == 0
                action
            }
            close?.let { enqueueClose(it) }
        }
        /** Worker-only wait. A non-last lease releases immediately; the last
         * waits for actual resource/ownership retirement, or reports its error. */
        fun awaitClosed() {
            val await = synchronized(lock) { check(closed); last }
            if (await) try { entry.closeAck.join() } catch (_: CompletionException) { throw Unavailable() }
        }
    }
    fun acquire(key: K, create: Boolean): Pending {
        val ticket: Ticket<K, T>
        val opening: Boolean
        val available: Boolean
        synchronized(lock) {
            if (users >= 128) throw Unavailable()
            val old = entries[key]
            if (old == null && entries.size >= 16) throw Unavailable()
            if (old != null && old.state != State.Opening && old.state != State.Open) throw Unavailable()
            val entry = old ?: Entry<K, T>(key, create).also { entries[key] = it }
            ticket = Ticket(entry)
            entry.users++; users++
            opening = old == null
            available = entry.state == State.Open
            if (available) ticket.state = TicketState.Available else entry.waiters.add(ticket)
        }
        if (opening) opener.execute { performOpen(ticket.entry) }
        if (available) ticket.signal.complete(null)
        return Pending(ticket)
    }
    private fun releaseLocked(entry: Entry<K, T>): Entry<K, T>? {
        check(entry.users > 0 && users > 0)
        entry.users--; users--
        if (entry.users != 0) return null
        if (entry.state != State.Retired) entry.state = State.Retiring
        return if (entry.resource != null && !entry.closeStarted) {
            entry.closeStarted = true
            entry
        } else null
    }
    private fun performOpen(entry: Entry<K, T>) {
        val abandoned = synchronized(lock) {
            if (entry.users == 0) { removeLocked(entry); true } else false
        }
        if (abandoned) { entry.closeAck.complete(null); return }
        var resource: T? = null
        try {
            resource = open(entry.key, entry.create)
            val close = synchronized(lock) {
                entry.resource = resource
                if (entry.users == 0 && !entry.closeStarted) { entry.closeStarted = true; entry } else null
            }
            // These callbacks update internal state and private signals only.
            // They never execute caller continuations under a registry monitor.
            closing(resource).whenComplete { _, _ -> synchronized(lock) { if (entry.state != State.Retired) entry.state = State.Retiring } }
            released(resource).whenComplete { _, error ->
                if (error == null) {
                    val acknowledged = synchronized(lock) { entry.ownershipReleased = true; removeLocked(entry); entry.closeSucceeded }
                    if (acknowledged) entry.closeAck.complete(null)
                }
            }
            close?.let { enqueueClose(it) }
            ready(resource).join()
            val waiters = synchronized(lock) {
                if (entry.state != State.Opening || entry.users == 0) throw Unavailable()
                entry.state = State.Open
                entry.waiters.toList().also { tickets ->
                    entry.waiters.clear()
                    tickets.forEach { it.state = TicketState.Available }
                }
            }
            waiters.forEach { it.signal.complete(null) }
        } catch (_: Exception) {
            var close: Entry<K, T>? = null
            val waiters = synchronized(lock) {
                val tickets = entry.waiters.toList(); entry.waiters.clear()
                if (resource == null) removeLocked(entry) else if (entry.state != State.Retired) entry.state = State.Retiring
                tickets.forEach { it.state = TicketState.Failed; close = releaseLocked(entry) ?: close }
                if (resource != null && entry.users == 0 && !entry.closeStarted) { entry.closeStarted = true; close = entry }
                tickets
            }
            waiters.forEach { it.signal.completeExceptionally(Unavailable()) }
            close?.let { enqueueClose(it) }
            if (resource == null) entry.closeAck.completeExceptionally(Unavailable())
        }
    }
    private fun enqueueClose(entry: Entry<K, T>) {
        closer.execute {
            try {
                retire(checkNotNull(entry.resource)).whenComplete { _, error ->
                    if (error != null) entry.closeAck.completeExceptionally(Unavailable()) else {
                        val acknowledged = synchronized(lock) { entry.closeSucceeded = true; entry.ownershipReleased }
                        if (acknowledged) entry.closeAck.complete(null)
                    }
                }
            } catch (_: Exception) { entry.closeAck.completeExceptionally(Unavailable()) }
        }
    }
    private fun removeLocked(entry: Entry<K, T>) {
        if (entries[entry.key] === entry) entries.remove(entry.key)
        entry.state = State.Retired
    }
    /** Test/process shutdown only after callers have retired every lease. */
    internal fun shutdownIdle() {
        synchronized(lock) { check(entries.isEmpty() && users == 0) }
        opener.shutdown(); closer.shutdown()
        check(opener.awaitTermination(30, TimeUnit.SECONDS))
        check(closer.awaitTermination(30, TimeUnit.SECONDS))
    }
}
