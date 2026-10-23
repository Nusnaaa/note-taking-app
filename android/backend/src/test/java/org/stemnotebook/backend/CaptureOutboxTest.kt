package org.stemnotebook.backend

import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.util.UUID

class CaptureOutboxTest {
    @get:Rule val temporary = TemporaryFolder()
    @Test fun restartKeepsExactRequestsAndOrderedRetry() {
        val directory = temporary.newFolder()
        val first = UUID.randomUUID()
        val journal = CaptureOutbox(directory)
        journal.enqueue(first, "first")
        journal.enqueue(UUID.randomUUID(), "second")
        // Equal mtime must not reorder records; ordering is in the durable name.
        directory.listFiles()!!.forEach { it.setLastModified(1) }
        journal.enqueue(first, "first")
        val restarted = CaptureOutbox(directory)
        val attempts = mutableListOf<String>()
        assertEquals(0, restarted.replay { attempts += it; false })
        assertEquals(listOf("first"), attempts)
        assertEquals(2, restarted.pendingCount())
        attempts.clear()
        assertEquals(2, restarted.replay { attempts += it; true })
        assertEquals(listOf("first", "second"), attempts)
        assertEquals(0, CaptureOutbox(directory).pendingCount())
    }
    @Test fun ambiguousCommitRetainsTicketAndConflictsNeverOverwrite() {
        val directory = temporary.newFolder()
        val journal = CaptureOutbox(directory)
        val ticket = UUID.randomUUID()
        journal.enqueue(ticket, "immutable")
        assertThrows(IllegalArgumentException::class.java) { journal.enqueue(ticket, "changed") }
        assertThrows(IllegalStateException::class.java) { journal.replay { error("lost reply") } }
        assertEquals(1, CaptureOutbox(directory).replay { assertEquals("immutable", it); true })
    }
    @Test fun fullJournalRejectsWithoutLosingAcceptedRecords() {
        val directory = temporary.newFolder()
        val journal = CaptureOutbox(directory, 6, 2)
        journal.enqueue(UUID.randomUUID(), "first")
        assertThrows(IllegalStateException::class.java) { journal.enqueue(UUID.randomUUID(), "too large") }
        assertEquals(1, journal.pendingCount())
        assertEquals(1, journal.replay { assertEquals("first", it); true })
        assertFalse(directory.listFiles()!!.any { it.name.endsWith(".pending") })
    }
    @Test fun encodingPreservesObservedValuesAndRejectsInvalidBatch() {
        val point = ObservedPoint(-0.0, 3.125, 2, null, 0.0, null, null)
        val stroke = CompletedStroke(UUID.randomUUID().toString(), UUID.randomUUID().toString(), 0, 1, false, listOf(point))
        val json = CaptureRequest.encode(UUID.randomUUID(), UUID.randomUUID(), "page\"\\\n", listOf(stroke))
        assertThrows(java.nio.charset.CharacterCodingException::class.java) {
            CaptureRequest.encode(UUID.randomUUID(), UUID.randomUUID(), 0xD800.toChar().toString(), listOf(stroke))
        }
        assertTrue(json.contains("\"x\":-0.0"))
        assertTrue(json.contains("\"pressure\":null"))
        assertTrue(json.contains("\"tilt_rad\":0.0"))
        assertTrue(json.contains("page\\\"\\\\\\u000a"))
        assertThrows(IllegalArgumentException::class.java) {
            CaptureRequest.encode(UUID.randomUUID(), UUID.randomUUID(), "page", listOf(stroke, stroke))
        }
        assertThrows(IllegalArgumentException::class.java) {
            CaptureRequest.encode(UUID.randomUUID(), UUID.randomUUID(), "page", listOf(stroke.copy(points = listOf(point.copy(x = Double.NaN)))))
        }
    }
}
