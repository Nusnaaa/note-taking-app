package org.stemnotebook.backend

import java.io.File
import java.io.FileOutputStream
import java.io.ByteArrayOutputStream
import java.nio.channels.FileChannel
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.nio.file.StandardOpenOption
import java.util.UUID

/** Durable, bounded admission before JNI. Run on the writer executor, never the
 * input thread. A ticket is immutable; an ambiguous reply is retried byte for byte.
 * This is a recovery journal, not a claim that an edit has reached SQLite. */
class CaptureOutbox(private val directory: File, private val maximumBytes: Long = 32L * 1024 * 1024, private val maximumBatches: Int = 256) {
    init {
        require(maximumBytes in 1..256L * 1024 * 1024 && maximumBatches in 1..4096)
        check(directory.isDirectory || directory.mkdirs())
        require(!Files.isSymbolicLink(directory.toPath()))
    }
    @Synchronized fun enqueue(ticket: UUID, request: String) {
        val bytes = request.toByteArray(Charsets.UTF_8)
        require(bytes.isNotEmpty() && bytes.size <= 16 * 1024 * 1024)
        val records = records()
        val existing = records.singleOrNull { it.name.endsWith("-$ticket.capture") }
        if (existing != null) {
            require(read(existing).contentEquals(bytes)) { "Capture ticket conflict" }
            return
        }
        check(records.size < maximumBatches && records.sumOf { it.length() } + bytes.size <= maximumBytes) { "Capture recovery journal full" }
        val lastSequence = records.lastOrNull()?.name?.substringBefore('-')?.toLong() ?: -1
        check(lastSequence < Long.MAX_VALUE)
        val destination = File(directory, "${(lastSequence + 1).toString().padStart(19, '0')}-$ticket.capture")
        val temporary = File.createTempFile("capture-", ".pending", directory)
        try {
            FileOutputStream(temporary).use { it.write(bytes); it.fd.sync() }
            Files.move(temporary.toPath(), destination.toPath(), StandardCopyOption.ATOMIC_MOVE)
            syncDirectory()
        } finally { temporary.delete() }
    }
    /** Failed and ambiguous replies retain the record. Stop on failure so retries
     * cannot reorder later observations ahead of an uncommitted earlier batch. */
    @Synchronized fun replay(send: (String) -> Boolean): Int {
        var committed = 0
        for (file in records()) {
            if (!send(read(file).toString(Charsets.UTF_8))) break
            check(file.delete()) { "Capture receipt cleanup failed" }
            syncDirectory()
            committed++
        }
        return committed
    }
    internal data class DeletionSnapshot(val stamps: List<DeletionCaptureStamp>, val unclassified: Int)
    private fun checksum(bytes: ByteArray) = java.security.MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it.toInt() and 255) }
    private fun deletionStamp(file: File): DeletionCaptureStamp? = try {
        val bytes = read(file)
        val raw = Charsets.UTF_8.newDecoder().onMalformedInput(java.nio.charset.CodingErrorAction.REPORT).onUnmappableCharacter(java.nio.charset.CodingErrorAction.REPORT).decode(java.nio.ByteBuffer.wrap(bytes)).toString()
        val request = org.json.JSONObject(raw); val command = request.getJSONObject("command")
        require(request.getInt("version") == 1 && command.getString("op") == "Capture")
        val ticket = UUID.fromString(command.getString("ticket")); val entity = UUID.fromString(command.getString("entity"))
        require(file.name.endsWith("-$ticket.capture"))
        val strokes = command.getJSONArray("strokes"); require(strokes.length() in 1..1000)
        val sealed = (0 until strokes.length()).all { index ->
            val stroke = strokes.getJSONObject(index)
            !stroke.has("end") || stroke.optString("end") == "Completed" || stroke.optJSONObject("end")?.optString("Interrupted") in listOf("Lifecycle", "Recovery")
        }
        DeletionCaptureStamp(DeletionCaptureStorage.LegacyOutbox, ticket, entity, checksum(bytes), sealed)
    } catch (_: Exception) { null } // Unknown records are retained, never attributed or discarded.
    @Synchronized internal fun deletionSnapshot(targets: Set<UUID>): DeletionSnapshot {
        var unclassified = 0
        val stamps = records().mapNotNull { file ->
            val stamp = deletionStamp(file)
            if (stamp == null) { unclassified++; null } else stamp.takeIf { it.entity in targets }
        }
        return DeletionSnapshot(stamps, unclassified)
    }
    @Synchronized internal fun discardForDeletion(stamp: DeletionCaptureStamp) {
        require(stamp.storage == DeletionCaptureStorage.LegacyOutbox && stamp.sealed)
        val file = records().singleOrNull { it.name.endsWith("-${stamp.ticket}.capture") } ?: throw java.io.IOException("Capture checkpoint unavailable")
        require(deletionStamp(file) == stamp)
        check(file.delete()); syncDirectory()
    }
    @Synchronized fun pendingCount(): Int = records().size
    private fun records(): List<File> {
        val files = directory.listFiles() ?: error("Capture recovery journal unavailable")
        return files.filter { it.name.endsWith(".capture") }.onEach {
            require(it.name.substringBefore('-').toLong() >= 0)
            UUID.fromString(it.name.substringAfter('-').removeSuffix(".capture"))
            require(it.isFile && !Files.isSymbolicLink(it.toPath()))
        }.sortedBy { it.name }.also { records ->
            require(records.size <= maximumBatches && records.sumOf { it.length() } <= maximumBytes)
        }
    }
    private fun read(file: File): ByteArray {
        require(!Files.isSymbolicLink(file.toPath()) && file.length() in 1..16L * 1024 * 1024)
        return file.inputStream().use {
            val out = ByteArrayOutputStream()
            val chunk = ByteArray(65536)
            while (true) {
                val count = it.read(chunk)
                if (count < 0) break
                require(out.size() + count <= 16 * 1024 * 1024)
                out.write(chunk, 0, count)
            }
            out.toByteArray()
        }
    }
    private fun syncDirectory() { FileChannel.open(directory.toPath(), StandardOpenOption.READ).use { it.force(true) } }
}

object CaptureRequest {
    /** Double.toString preserves a measured negative zero and avoids float casts. */
    fun encode(ticket: UUID, entity: UUID, coordinateSpace: String, strokes: List<CompletedStroke>, layer: CaptureLayer? = null): String {
        val encodedSpace = Charsets.UTF_8.newEncoder().onMalformedInput(java.nio.charset.CodingErrorAction.REPORT).onUnmappableCharacter(java.nio.charset.CodingErrorAction.REPORT).encode(java.nio.CharBuffer.wrap(coordinateSpace))
        require(coordinateSpace.isNotEmpty() && encodedSpace.remaining() <= 256 && strokes.isNotEmpty())
        fun quote(value: String): String = buildString {
            append('"')
            for (c in value) when (c) {
                '"' -> append("\\\""); '\\' -> append("\\\\")
                else -> if (c.code < 32) append("\\u%04x".format(c.code)) else append(c)
            }
            append('"')
        }
        fun number(value: Double?): String { require(value == null || value.isFinite()); return value?.toString() ?: "null" }
        val encoded = strokes.map { stroke ->
            val ending = when (val end = stroke.end) {
                StrokeEnd.Completed -> ""
                StrokeEnd.InProgress -> throw IllegalArgumentException("An active prefix is not a completed capture")
                is StrokeEnd.Interrupted -> ",\"end\":{\"Interrupted\":\"${end.reason.name}\"}"
            }
            val style = stroke.style?.let { ",\"style\":{\"colour_rgba\":[${it.red},${it.green},${it.blue},${it.alpha}],\"width\":${number(it.width)}}" } ?: ""
            UUID.fromString(stroke.id); UUID.fromString(stroke.session)
            require(stroke.sequence >= 0 && stroke.pointer >= 0 && stroke.points.size in 1..100_000)
            var previous = 0L
            val points = stroke.points.map { point ->
                require(point.timeUs >= previous && (point.buttons == null || point.buttons >= 0))
                previous = point.timeUs
                "{\"x\":${number(point.x)},\"y\":${number(point.y)},\"time_us\":${point.timeUs},\"pressure\":${number(point.pressure)},\"tilt_rad\":${number(point.tiltRad)},\"orientation_rad\":${number(point.orientationRad)},\"buttons\":${point.buttons ?: "null"}}"
            }.joinToString(",")
            "{\"id\":${quote(stroke.id)},\"capture_session\":${quote(stroke.session)},\"sequence\":${stroke.sequence},\"pointer_id\":${stroke.pointer},\"coordinate_space\":${quote(coordinateSpace)},\"tool\":\"${if (stroke.eraser) "Eraser" else "Pen"}\",\"points\":[$points]$ending$style}"
        }.joinToString(",")
        require(strokes.map { it.id }.toSet().size == strokes.size)
        if (layer != null) {
            val target = org.json.JSONObject().put("CanvasLayer", org.json.JSONObject().put("canvas", entity.toString()).put("layer", layer.wire()))
            return "{\"version\":1,\"command\":{\"op\":\"CaptureTo\",\"ticket\":\"$ticket\",\"target\":$target,\"strokes\":[$encoded]}}"
        }
        return "{\"version\":1,\"command\":{\"op\":\"Capture\",\"ticket\":\"$ticket\",\"entity\":\"$entity\",\"strokes\":[$encoded]}}"
    }
}
