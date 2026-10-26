package org.stemnotebook.backend

import java.io.*
import java.nio.channels.FileChannel
import java.nio.charset.CodingErrorAction
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.nio.file.StandardOpenOption
import java.security.MessageDigest
import java.util.UUID

/** Private recovery storage, not document persistence. Caller serialises all access.
 * Every authoritative record is immutable-by-revision and checksum protected. */
internal class ActiveCaptureJournal(
    private val directory: File,
    private val library: UUID,
    private val maximumBytes: Long = 32L * 1024 * 1024,
    private val maximumRecords: Int = 256,
    private val fault: (String) -> Unit = {},
) {
    internal data class Entry(val ordinal: Long, val library: UUID, val entity: UUID, val space: String, val revision: Long, val stroke: CapturedStroke, val cancelled: Boolean = false, val review: Boolean = false, val layer: CaptureLayer? = null) {
        val ticket: UUID get() = UUID.fromString(stroke.id)
    }
    private var failed = false
    init {
        require(maximumBytes in 1..256L * 1024 * 1024 && maximumRecords in 1..4096)
        val existed = directory.isDirectory
        check(existed || directory.mkdirs()); require(!Files.isSymbolicLink(directory.toPath()))
        if (!existed) {
            syncDirectory()
            directory.absoluteFile.parentFile?.let { parent -> FileChannel.open(parent.toPath(), StandardOpenOption.READ).use { it.force(true) } }
        }
        validateFiles()
        recoverTemporaries()
        validateFiles()
    }
    fun contains(ticket: UUID): Boolean = fileFor(ticket) != null
    fun pendingCount(): Int = records().size
    fun reviewTickets(): List<UUID> = records().map { decode(readBytes(it)) }.filter { it.stroke.end != StrokeEnd.InProgress && !it.cancelled }.map { it.ticket }
    fun reviewCount(): Int = records().count { decode(readBytes(it)).review }
    fun incompleteCount(): Int = ownFiles().count { it.name.endsWith(".incomplete") }
    fun read(ticket: UUID): Entry = decode(readBytes(fileFor(ticket) ?: throw IOException("Capture checkpoint unavailable"))).also { require(it.library == library) }
    fun deletionSnapshot(targets: Set<UUID>): List<DeletionCaptureStamp> = records().mapNotNull { file ->
        val entry = decode(readBytes(file))
        if (entry.entity !in targets || entry.cancelled) null
        else DeletionCaptureStamp(DeletionCaptureStorage.ActiveJournal, entry.ticket, entry.entity, checksum(entry.ticket), entry.stroke.end != StrokeEnd.InProgress)
    }
    fun checksum(ticket: UUID): String = MessageDigest.getInstance("SHA-256").digest(readBytes(checkNotNull(fileFor(ticket)))).joinToString("") { "%02x".format(it.toInt() and 255) }
    fun checkpoint(entity: UUID, space: String, revision: Long, stroke: CapturedStroke, cancelled: Boolean = false, layer: CaptureLayer? = null): Entry {
        check(!failed)
        val ticket = UUID.fromString(stroke.id); val existing = fileFor(ticket)?.let { decode(readBytes(it)) }
        val ordinal = existing?.ordinal ?: records().lastOrNull()?.name?.substringBefore('-')?.toLong()?.let { check(it < Long.MAX_VALUE); it + 1 } ?: 0L
        val proposed = Entry(ordinal, library, entity, space, revision, stroke, cancelled, stroke.end == StrokeEnd.Interrupted(StrokeInterruption.Recovery), layer)
        validate(proposed)
        if (existing != null) validateExtension(existing, proposed)
        publish(proposed)
        return proposed
    }
    /** A surviving active prefix is reviewable evidence, never an invented completion. */
    fun recoverActive() {
        for (file in records()) {
            val entry = decode(readBytes(file))
            if (entry.cancelled) { remove(file); continue }
            if (entry.stroke.end == StrokeEnd.InProgress) {
                check(entry.revision < Long.MAX_VALUE)
                publish(entry.copy(revision = entry.revision + 1, stroke = entry.stroke.copy(end = StrokeEnd.Interrupted(StrokeInterruption.Recovery)), review = true))
            }
        }
    }
    fun approve(ticket: UUID, expectedChecksum: String) {
        require(checksum(ticket) == expectedChecksum)
        val entry = read(ticket); require(entry.stroke.end != StrokeEnd.InProgress && !entry.cancelled)
        if (entry.review) {
            require(entry.revision < Long.MAX_VALUE)
            publish(entry.copy(revision = entry.revision + 1, review = false))
        }
    }
    /** Only an explicitly confirmed, still-uncommitted routing decision may call
     * this. Observations/identity/order remain unchanged; ordinary checkpoints
     * still cannot rebind a destination. Native receipt checks belong to the writer. */
    fun retarget(ticket: UUID, expectedChecksum: String, layer: CaptureLayer): Entry {
        require(checksum(ticket) == expectedChecksum)
        val entry = read(ticket)
        require(entry.stroke.end != StrokeEnd.InProgress && !entry.cancelled && entry.layer != layer)
        check(entry.revision < Long.MAX_VALUE)
        return entry.copy(revision = entry.revision + 1, layer = layer).also { publish(it) }
    }
    fun discardReview(ticket: UUID, expectedChecksum: String, beforeRemoval: () -> Unit = {}) {
        require(checksum(ticket) == expectedChecksum)
        val entry = read(ticket); require(entry.stroke.end != StrokeEnd.InProgress && !entry.cancelled)
        // Retire typed caller frames before unlink: directory-sync failure must
        // not let an unacknowledged removal recreate the same private evidence.
        beforeRemoval()
        remove(checkNotNull(fileFor(ticket)))
    }
    /** Preserve each session's order; blocked recovery never stalls unrelated input. */
    fun drain(send: (Entry) -> Boolean): Int {
        var committed = 0
        val blocked = mutableSetOf<String>()
        for (file in records()) {
            val entry = decode(readBytes(file))
            if (entry.cancelled) { remove(file); continue }
            if (entry.stroke.session in blocked) continue
            if (entry.review || entry.stroke.end == StrokeEnd.InProgress || !send(entry)) { blocked += entry.stroke.session; continue }
            fault("after-native-reply")
            remove(file); committed++
        }
        return committed
    }
    private fun validateExtension(old: Entry, next: Entry) {
        require(old.ordinal == next.ordinal && old.library == next.library && old.entity == next.entity && old.space == next.space && old.layer == next.layer)
        require(old.stroke.copy(points = emptyList(), end = StrokeEnd.InProgress) == next.stroke.copy(points = emptyList(), end = StrokeEnd.InProgress))
        require(next.revision >= old.revision)
        if (next.revision == old.revision) { require(old == next); return }
        require(!old.cancelled && old.stroke.end == StrokeEnd.InProgress)
        require(next.stroke.points.size >= old.stroke.points.size && next.stroke.points.take(old.stroke.points.size) == old.stroke.points)
    }
    private fun publish(entry: Entry) {
        check(!failed); validate(entry)
        val destination = File(directory, name(entry)); val bytes = encode(entry)
        val files = records(); require(files.size + (if (destination.exists()) 0 else 1) <= maximumRecords)
        require(ownFiles().sumOf { it.length() } - (if (destination.exists()) destination.length() else 0L) + bytes.size <= maximumBytes)
        val temporary = File.createTempFile("active-", ".pending", directory)
        try {
            FileOutputStream(temporary).use { it.write(bytes); it.fd.sync() }
            fault("after-file-sync")
            Files.move(temporary.toPath(), destination.toPath(), StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE)
            fault("after-rename"); syncDirectory(); fault("after-directory-sync")
        } finally {
            if (temporary.exists() && !temporary.delete()) { failed = true; throw IOException("Capture checkpoint cleanup failed") }
        }
    }
    private fun recoverTemporaries() {
        val temporary = ownFiles().filter { it.name.endsWith(".pending") }
        for (file in temporary) {
            val candidate = try { decode(readBytes(file)) } catch (_: Exception) {
                Files.move(file.toPath(), File(directory, file.name.removeSuffix(".pending") + ".incomplete").toPath(), StandardCopyOption.ATOMIC_MOVE)
                syncDirectory(); continue
            }
            require(candidate.library == library) { "Capture checkpoint belongs to another library" }
            val old = fileFor(candidate.ticket)?.let { decode(readBytes(it)) }
            if (old != null) {
                if (candidate.revision < old.revision) { validateExtension(candidate, old); remove(file); continue }
                validateExtension(old, candidate)
            }
            Files.move(file.toPath(), File(directory, name(candidate)).toPath(), StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE)
            syncDirectory()
        }
    }
    private fun remove(file: File) { fault("before-delete"); check(file.delete()); fault("after-delete"); syncDirectory() }
    private fun name(entry: Entry) = "${entry.ordinal.toString().padStart(19, '0')}-${entry.ticket}.prefix"
    private fun fileFor(ticket: UUID): File? = records().singleOrNull { it.name.endsWith("-$ticket.prefix") }
    private fun ownFiles(): List<File> = (directory.listFiles() ?: throw IOException("Capture checkpoint storage unavailable")).filter {
        it.name.endsWith(".prefix") || (it.name.startsWith("active-") && (it.name.endsWith(".pending") || it.name.endsWith(".incomplete")))
    }.onEach { require(it.isFile && !Files.isSymbolicLink(it.toPath())) }
    private fun validateFiles() {
        val files = ownFiles(); require(files.size <= maximumRecords * 2 + 1 && files.sumOf { it.length() } <= maximumBytes + MAX_RECORD)
        val ids = mutableSetOf<UUID>(); val ordinals = mutableSetOf<Long>()
        for (file in records()) {
            val entry = decode(readBytes(file)); require(entry.library == library && file.name == name(entry) && ids.add(entry.ticket) && ordinals.add(entry.ordinal))
        }
    }
    private fun records(): List<File> = ownFiles().filter { it.name.endsWith(".prefix") }.sortedBy { it.name }.also { require(it.size <= maximumRecords) }
    private fun syncDirectory() { FileChannel.open(directory.toPath(), StandardOpenOption.READ).use { it.force(true) } }
    private fun readBytes(file: File): ByteArray {
        require(!Files.isSymbolicLink(file.toPath()) && file.length() in 40..MAX_RECORD)
        return file.inputStream().use { input ->
            val bytes = ByteArrayOutputStream(); val buffer = ByteArray(65536)
            while (true) { val n = input.read(buffer); if (n < 0) break; require(bytes.size() + n <= MAX_RECORD); bytes.write(buffer, 0, n) }
            bytes.toByteArray()
        }
    }
    companion object {
        private const val MAX_RECORD = 6L * 1024 * 1024
        private val LEGACY_MAGIC = "STEMAC01".toByteArray(Charsets.US_ASCII)
        private val STYLE_MAGIC = "STEMAC02".toByteArray(Charsets.US_ASCII)
        private val LAYER_MAGIC = "STEMAC03".toByteArray(Charsets.US_ASCII)
        private val FINGER_MAGIC = "STEMAC04".toByteArray(Charsets.US_ASCII)
        private val MAGIC = "STEMAC05".toByteArray(Charsets.US_ASCII)
        private fun validate(entry: Entry) {
            require(entry.ordinal >= 0 && entry.revision >= 0 && entry.stroke.sequence >= 0 && entry.stroke.pointer >= 0)
            UUID.fromString(entry.stroke.id); UUID.fromString(entry.stroke.session)
            val encodedSpace = Charsets.UTF_8.newEncoder().onMalformedInput(CodingErrorAction.REPORT).onUnmappableCharacter(CodingErrorAction.REPORT).encode(java.nio.CharBuffer.wrap(entry.space))
            require(entry.space.isNotEmpty() && encodedSpace.remaining() <= 256 && entry.stroke.points.size in 1..100_000)
            var previous = 0L
            for (p in entry.stroke.points) {
                require(p.x.isFinite() && p.y.isFinite() && p.timeUs >= previous && (p.buttons == null || p.buttons >= 0))
                require(listOf(p.pressure, p.tiltRad, p.orientationRad).filterNotNull().all { it.isFinite() }); previous = p.timeUs
            }
            require(!entry.cancelled || (entry.stroke.end == StrokeEnd.InProgress && !entry.review))
            require(!entry.review || entry.stroke.end == StrokeEnd.Interrupted(StrokeInterruption.Recovery))
        }
        private fun encode(entry: Entry): ByteArray {
            val buffer = ByteArrayOutputStream()
            DataOutputStream(buffer).use { out ->
                fun uuid(value: UUID) { out.writeLong(value.mostSignificantBits); out.writeLong(value.leastSignificantBits) }
                out.write(MAGIC); out.writeLong(entry.ordinal); uuid(entry.library); uuid(entry.entity); out.writeLong(entry.revision)
                val s = entry.stroke; uuid(UUID.fromString(s.id)); uuid(UUID.fromString(s.session)); out.writeLong(s.sequence); out.writeInt(s.pointer); out.writeByte(if (s.explicitFinger) 2 else if (s.eraser) 1 else 0)
                out.writeByte(when (s.end) { StrokeEnd.InProgress -> 0; StrokeEnd.Completed -> 1; is StrokeEnd.Interrupted -> when (s.end.reason) { StrokeInterruption.Lifecycle -> 2; StrokeInterruption.Recovery -> 3 } })
                out.writeBoolean(entry.cancelled); out.writeBoolean(entry.review)
                out.writeBoolean(s.style != null)
                s.style?.let {
                    out.writeByte(it.red); out.writeByte(it.green); out.writeByte(it.blue); out.writeByte(it.alpha); out.writeLong(it.width.toRawBits())
                    out.writeBoolean(it.pressureResponse != null)
                    it.pressureResponse?.let { r -> listOf(r.inputMin, r.inputMax, r.minimumFactor, r.maximumFactor, r.exponent).forEach { value -> out.writeLong(value.toRawBits()) } }
                }
                when (val layer = entry.layer) {
                    null -> out.writeByte(0)
                    CaptureLayer.Base -> out.writeByte(1)
                    is CaptureLayer.Custom -> { out.writeByte(2); uuid(layer.id) }
                }
                val space = entry.space.toByteArray(Charsets.UTF_8); out.writeInt(space.size); out.write(space); out.writeInt(s.points.size)
                for (p in s.points) {
                    out.writeLong(p.x.toRawBits()); out.writeLong(p.y.toRawBits()); out.writeLong(p.timeUs)
                    var flags = 0; if (p.pressure != null) flags = flags or 1; if (p.tiltRad != null) flags = flags or 2; if (p.orientationRad != null) flags = flags or 4; if (p.buttons != null) flags = flags or 8
                    out.writeByte(flags); p.pressure?.let { out.writeLong(it.toRawBits()) }; p.tiltRad?.let { out.writeLong(it.toRawBits()) }; p.orientationRad?.let { out.writeLong(it.toRawBits()) }; p.buttons?.let { out.writeInt(it) }
                }
            }
            val payload = buffer.toByteArray(); return payload + MessageDigest.getInstance("SHA-256").digest(payload)
        }
        private fun decode(bytes: ByteArray): Entry {
            require(bytes.size in 40..MAX_RECORD.toInt()); val payload = bytes.copyOfRange(0, bytes.size - 32)
            require(MessageDigest.isEqual(bytes.copyOfRange(bytes.size - 32, bytes.size), MessageDigest.getInstance("SHA-256").digest(payload)))
            val input = DataInputStream(ByteArrayInputStream(payload))
            fun uuid() = UUID(input.readLong(), input.readLong())
            fun flag(): Boolean { val value = input.readUnsignedByte(); require(value in 0..1); return value == 1 }
            val magic = ByteArray(8).also { input.readFully(it) }; require(magic.contentEquals(MAGIC) || magic.contentEquals(FINGER_MAGIC) || magic.contentEquals(LAYER_MAGIC) || magic.contentEquals(STYLE_MAGIC) || magic.contentEquals(LEGACY_MAGIC))
            val ordinal = input.readLong(); val library = uuid(); val entity = uuid(); val revision = input.readLong(); val id = uuid(); val session = uuid(); val sequence = input.readLong(); val pointer = input.readInt(); val tool = input.readUnsignedByte(); require(tool in 0..(if (magic.contentEquals(MAGIC) || magic.contentEquals(FINGER_MAGIC)) 2 else 1)); val eraser = tool == 1
            val end = when (input.readUnsignedByte()) { 0 -> StrokeEnd.InProgress; 1 -> StrokeEnd.Completed; 2 -> StrokeEnd.Interrupted(StrokeInterruption.Lifecycle); 3 -> StrokeEnd.Interrupted(StrokeInterruption.Recovery); else -> throw IOException("Capture checkpoint state invalid") }
            val cancelled = flag(); val review = flag()
            val style = if (!magic.contentEquals(LEGACY_MAGIC) && flag()) {
                val red = input.readUnsignedByte(); val green = input.readUnsignedByte(); val blue = input.readUnsignedByte(); val alpha = input.readUnsignedByte(); val width = Double.fromBits(input.readLong())
                val response = if (magic.contentEquals(MAGIC) && flag()) PressureResponse(Double.fromBits(input.readLong()), Double.fromBits(input.readLong()), Double.fromBits(input.readLong()), Double.fromBits(input.readLong()), Double.fromBits(input.readLong())) else null
                StrokeStyle(red, green, blue, alpha, width, response)
            } else null
            val layer = if (magic.contentEquals(MAGIC) || magic.contentEquals(FINGER_MAGIC) || magic.contentEquals(LAYER_MAGIC)) when (input.readUnsignedByte()) {
                0 -> null; 1 -> CaptureLayer.Base; 2 -> CaptureLayer.Custom(uuid()); else -> throw IOException("Capture destination invalid")
            } else null
            val length = input.readInt(); require(length in 1..256)
            val encoded = ByteArray(length).also { input.readFully(it) }; val space = Charsets.UTF_8.newDecoder().onMalformedInput(CodingErrorAction.REPORT).onUnmappableCharacter(CodingErrorAction.REPORT).decode(java.nio.ByteBuffer.wrap(encoded)).toString()
            val count = input.readInt(); require(count in 1..100_000)
            val points = List(count) {
                val x = Double.fromBits(input.readLong()); val y = Double.fromBits(input.readLong()); val time = input.readLong(); val flags = input.readUnsignedByte(); require(flags in 0..15)
                ObservedPoint(x, y, time, if (flags and 1 != 0) Double.fromBits(input.readLong()) else null, if (flags and 2 != 0) Double.fromBits(input.readLong()) else null, if (flags and 4 != 0) Double.fromBits(input.readLong()) else null, if (flags and 8 != 0) input.readInt() else null)
            }
            require(input.available() == 0)
            return Entry(ordinal, library, entity, space, revision, CapturedStroke(id.toString(), session.toString(), sequence, pointer, eraser, points, end, style, tool == 2), cancelled, review, layer).also { validate(it) }
        }
    }
}
