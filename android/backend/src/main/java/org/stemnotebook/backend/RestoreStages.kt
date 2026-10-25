package org.stemnotebook.backend

import android.system.Os
import org.json.JSONObject
import java.io.File
import java.io.IOException
import java.nio.ByteBuffer
import java.nio.channels.FileChannel
import java.nio.file.Files
import java.nio.file.LinkOption
import java.nio.file.Path
import java.nio.file.StandardCopyOption
import java.nio.file.StandardOpenOption
import java.nio.file.attribute.PosixFilePermissions
import java.security.MessageDigest
import java.util.UUID

/** Owned scratch only. Payloads are statted, never opened as raw SQLite FDs.
 * The app/native restore reservation must span inspection and mutations. */
internal class RestoreStages(private val pending: File, private val retired: File, private val fault: (String) -> Unit = {}) {
    companion object {
        private const val STAGE = "stage.json"
        private const val RETIREMENT = "retirement.json"
        private const val MARKER_LIMIT = 16384
        private val nativeFile = Regex("^\\.stem-stream-restore-([0-9a-f-]{36})\\.db(?:-wal|-shm)?$")
    }
    internal data class Identity(val device: Long, val inode: Long, val links: Long, val bytes: Long,
        val modified: String, val changed: String, val mode: Int, val uid: Int) {
        val directory get() = mode and 0xf000 == 0x4000
        val regular get() = mode and 0xf000 == 0x8000
        val private get() = mode and 0x3f == 0
        fun sameMovedFile(other: Identity) = device == other.device && inode == other.inode && links == other.links && bytes == other.bytes && mode == other.mode && uid == other.uid && modified == other.modified
    }
    internal data class Stage(val directory: File, val identity: Identity, val marker: ByteArray)
    internal data class Child(val name: String, val identity: Identity)
    internal data class Entry(val name: String, val retired: Boolean)
    internal data class Page(val entries: List<Entry>, val next: String?)
    internal data class Snapshot(val entry: Entry, val root: File, val identity: Identity, val children: List<Child>,
        val markerDigest: String?, val recognised: Boolean, val observedBytes: Long?, val complete: Boolean,
        val originalName: String, val container: File? = null, val envelope: String? = null, val containerIdentity: Identity? = null, val envelopeIdentity: Identity? = null, val metadataOnly: Boolean = false, val emptyDirectory: Boolean = false)
    internal enum class Disposition { DiscardRecognised, Quarantine }
    internal enum class Outcome { Discarded, Quarantined, Uncertain }
    internal data class Receipt(val outcome: Outcome, val retirement: UUID, val sourceRetired: Boolean, val removedBytes: Long)
    internal class Changed : IOException("Restore staging changed")
    private val pendingIdentity: Identity
    private val retiredIdentity: Identity
    init {
        privateDirectory(pending); privateDirectory(retired)
        pendingIdentity = identity(pending.toPath()); retiredIdentity = identity(retired.toPath())
    }
    private fun checkRoots() {
        for ((root, before) in listOf(pending to pendingIdentity, retired to retiredIdentity)) {
            val current = identity(root.toPath())
            if (!current.directory || current.device != before.device || current.inode != before.inode || current.mode != before.mode || current.uid != before.uid)
                throw Changed()
        }
    }

    fun hasPending(): Boolean { checkRoots(); return Files.newDirectoryStream(pending.toPath()).use { it.iterator().hasNext() } }
    fun hasRetired(): Boolean { checkRoots(); return Files.newDirectoryStream(retired.toPath()).use { it.iterator().hasNext() } }
    fun create(library: String): Stage {
        checkRoots()
        val id = UUID.randomUUID()
        val root = File(pending, id.toString())
        Files.createDirectory(root.toPath(), PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rwx------")))
        val body = JSONObject().put("format", 1).put("purpose", "restore-stage").put("library", library)
            .put("stage", id.toString()).put("created_at_millis", System.currentTimeMillis()).toString().toByteArray(Charsets.UTF_8)
        writeNew(File(root, STAGE), body); sync(root); sync(pending)
        return Stage(root, identity(root.toPath()), body)
    }
    fun retire(stage: Stage) {
        checkRoots()
        val current = identity(stage.directory.toPath())
        require(current.device == stage.identity.device && current.inode == stage.identity.inode && current.directory && current.private && current.uid == stage.identity.uid)
        val names = Files.newDirectoryStream(stage.directory.toPath()).use { it.map { path -> path.fileName.toString() }.take(2) }
        require(names == listOf(STAGE))
        require(marker(File(stage.directory, STAGE), current.uid)?.toByteArray(Charsets.UTF_8)?.contentEquals(stage.marker) == true)
        Files.delete(File(stage.directory, STAGE).toPath()); Files.delete(stage.directory.toPath()); sync(pending)
    }
    fun page(isRetired: Boolean, after: String?, limit: Int, check: () -> Unit): Page {
        checkRoots()
        val folder = if (isRetired) retired else pending
        val selected = java.util.TreeSet<String>()
        Files.newDirectoryStream(folder.toPath()).use { entries ->
            for (entry in entries) {
                check()
                val name = entry.fileName.toString()
                if (after != null && name <= after) continue
                selected += name
                if (selected.size > limit + 1) selected.pollLast()
            }
        }
        val names = selected.take(limit)
        return Page(names.map { Entry(it, isRetired) }, if (selected.size > limit) names.lastOrNull() else null)
    }
    fun capture(entry: Entry, library: String, maximumFiles: Int, check: () -> Unit): Snapshot {
        checkRoots()
        require(validName(entry.name))
        val folder = if (entry.retired) retired else pending
        val root = File(folder, entry.name)
        if (!entry.retired) return inspect(entry, root, entry.name, library, maximumFiles, check)
        // Retired containers are opaque until their private bounded envelope is verified.
        val parentIdentity = identity(root.toPath())
        val envelope = if (parentIdentity.directory && parentIdentity.private) marker(File(root, RETIREMENT), parentIdentity.uid) else null
        val data = envelope?.let { runCatching { JSONObject(it) }.getOrNull() }
        val verified = retirementMarker(data, library, entry.name)
        if (!verified) return inspect(entry, root, entry.name, library, maximumFiles, check).let { it.copy(recognised = it.emptyDirectory) }
        val original = checkNotNull(data).getString("original")
        val child = File(root, "entry")
        val names = Files.newDirectoryStream(root.toPath()).use { it.map { path -> path.fileName.toString() }.take(3).toSet() }
        if (names == setOf(RETIREMENT)) {
            // Crash after the scratch was deleted: only this verified protocol record remains.
            val result = inspect(entry, root, entry.name, library, maximumFiles, check)
            if (identity(root.toPath()) != parentIdentity) throw Changed()
            return result
        }
        if (names != setOf(RETIREMENT, "entry")) return inspect(entry, root, entry.name, library, maximumFiles, check).let { it.copy(recognised = it.emptyDirectory) }
        val result = inspect(entry, child, original, library, maximumFiles, check)
        if (identity(root.toPath()) != parentIdentity || marker(File(root, RETIREMENT), parentIdentity.uid) != envelope) throw Changed()
        return result.copy(container = root, envelope = envelope, containerIdentity = parentIdentity, envelopeIdentity = identity(File(root, RETIREMENT).toPath()))
    }
    private fun inspect(entry: Entry, root: File, original: String, library: String, maximumFiles: Int, check: () -> Unit): Snapshot {
        check()
        val before = identity(root.toPath())
        var complete = true
        val children = if (before.directory) Files.newDirectoryStream(root.toPath()).use { paths ->
            val names = java.util.TreeSet<String>()
            for (path in paths) {
                check(); names += path.fileName.toString()
                if (names.size > maximumFiles) { complete = false; names.pollLast() }
            }
            names.map { name -> check(); Child(name, identity(File(root, name).toPath())) }
        } else emptyList()
        val text = if (before.directory && before.private) marker(File(root, STAGE), before.uid) else null
        val data = text?.let { runCatching { JSONObject(it) }.getOrNull() }
        val id = runCatching { UUID.fromString(original).toString() }.getOrNull()
        var recognised = complete && id == original && stageMarker(data, library, original)
        val metadata = if (complete && children.map { it.name }.toSet() == setOf(RETIREMENT) && before.directory && before.private)
            marker(File(root, RETIREMENT), before.uid) else null
        val retirement = metadata?.let { runCatching { JSONObject(it) }.getOrNull() }
        val metadataOnly = id == original && retirementMarker(retirement, library, original)
        val empty = before.directory && before.private && complete && children.isEmpty()
        recognised = recognised || metadataOnly || empty
        val bases = mutableSetOf<String>()
        for (child in children) {
            if (!child.identity.regular || !child.identity.private || child.identity.links != 1L || child.identity.uid != before.uid) recognised = false
            if (child.name != STAGE && !(metadataOnly && child.name == RETIREMENT)) {
                val match = nativeFile.matchEntire(child.name)
                if (match == null || runCatching { UUID.fromString(match.groupValues[1]).toString() }.getOrNull() != match.groupValues[1]) recognised = false
                else bases += match.groupValues[1]
            }
        }
        if (bases.size > 1 || (!metadataOnly && !empty && children.none { it.name == STAGE })) recognised = false
        if (identity(root.toPath()) != before) throw Changed()
        val bytes = if (before.directory && complete && children.all { it.identity.regular }) runCatching { children.fold(0L) { sum, child -> Math.addExact(sum, child.identity.bytes) } }.getOrNull()
            else if (!before.directory && before.regular) before.bytes else null
        return Snapshot(entry, root, before, children, (text ?: metadata)?.let { digest(it.toByteArray(Charsets.UTF_8)) }, recognised, bytes, complete, original, metadataOnly = metadataOnly, emptyDirectory = empty)
    }
    /** The caller supplies current app authority/fresh proof and owns the native
     * reservation. onCommit must switch observation to an outcome before rename. */
    fun act(snapshot: Snapshot, library: String, maximumFiles: Int, disposition: Disposition, check: () -> Unit, onCommit: () -> Unit): Receipt {
        if (disposition == Disposition.DiscardRecognised) require(snapshot.recognised)
        if (snapshot.entry.retired && !snapshot.recognised && disposition == Disposition.Quarantine)
            throw IOException("Unclassified data is already quarantined")
        check()
        if (capture(snapshot.entry, library, maximumFiles, check) != snapshot) throw Changed()
        val id = UUID.randomUUID()
        val target = File(retired, id.toString())
        Files.createDirectory(target.toPath(), PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rwx------")))
        val body = JSONObject().put("format", 1).put("purpose", "restore-retirement").put("library", library).put("id", id.toString())
            .put("original", snapshot.originalName).put("disposition", disposition.name).put("created_at_millis", System.currentTimeMillis())
        var entered = false
        var moved = false
        var removed = 0L
        try {
            writeNew(File(target, RETIREMENT), body.toString().toByteArray(Charsets.UTF_8)); sync(target); sync(retired)
            fault("retirement-before-move"); check()
            if (capture(snapshot.entry, library, maximumFiles, check) != snapshot) throw Changed()
            checkRoots(); onCommit(); entered = true
            Files.move(snapshot.root.toPath(), File(target, "entry").toPath(), StandardCopyOption.ATOMIC_MOVE)
            moved = true
            sync(snapshot.root.parentFile!!); sync(target)
            fault("retirement-moved"); check()
            val transferred = inspect(snapshot.entry, File(target, "entry"), snapshot.originalName, library, maximumFiles, check)
            if (!transferred.identity.sameMovedFile(snapshot.identity) || transferred.children != snapshot.children ||
                transferred.markerDigest != snapshot.markerDigest || transferred.complete != snapshot.complete) throw Changed()
            if (disposition == Disposition.DiscardRecognised) {
                if (!snapshot.identity.directory) throw Changed()
                val files = transferred.children.sortedBy { it.name == STAGE || it.name == RETIREMENT }
                for (child in files) {
                    check(); fault("retirement-before-delete")
                    val path = File(target, "entry/${child.name}").toPath()
                    if (identity(path) != child.identity) throw Changed()
                    Files.delete(path); removed = Math.addExact(removed, child.identity.bytes)
                }
                check(); Files.delete(File(target, "entry").toPath()); sync(target)
            }
            prunePreviousContainer(snapshot)
            if (disposition == Disposition.DiscardRecognised) {
                fault("retirement-before-record-cleanup"); check()
                Files.delete(File(target, RETIREMENT).toPath()); Files.delete(target.toPath()); sync(retired)
                return Receipt(Outcome.Discarded, id, true, removed)
            }
            return Receipt(Outcome.Quarantined, id, true, 0)
        } catch (error: Throwable) {
            if (!entered) {
                // Only our fresh, empty reservation and marker; source is untouched.
                try { Files.deleteIfExists(File(target, RETIREMENT).toPath()); Files.delete(target.toPath()); sync(retired) } catch (_: Exception) { }
                throw error
            }
            return Receipt(Outcome.Uncertain, id, moved, removed)
        }
    }
    private fun prunePreviousContainer(snapshot: Snapshot) {
        val container = snapshot.container ?: return
        val current = identity(container.toPath())
        val before = checkNotNull(snapshot.containerIdentity)
        if (current.device != before.device || current.inode != before.inode || current.mode != before.mode || current.uid != before.uid ||
            identity(File(container, RETIREMENT).toPath()) != snapshot.envelopeIdentity) throw Changed()
        val names = Files.newDirectoryStream(container.toPath()).use { it.map { path -> path.fileName.toString() }.take(2) }
        if (names != listOf(RETIREMENT) || marker(File(container, RETIREMENT), identity(container.toPath()).uid) != snapshot.envelope) throw Changed()
        Files.delete(File(container, RETIREMENT).toPath()); Files.delete(container.toPath()); sync(retired)
    }
    private fun marker(file: File, uid: Int): String? {
        if (!Files.exists(file.toPath(), LinkOption.NOFOLLOW_LINKS)) return null
        val before = identity(file.toPath())
        if (!before.regular || !before.private || before.links != 1L || before.uid != uid || before.bytes !in 1..MARKER_LIMIT) return null
        val bytes = readSmall(file)
        if (identity(file.toPath()) != before) throw Changed()
        return bytes.toString(Charsets.UTF_8)
    }
    private fun readSmall(file: File): ByteArray {
        val buffer = ByteArray(MARKER_LIMIT + 1)
        var count = 0
        Files.newInputStream(file.toPath(), StandardOpenOption.READ, LinkOption.NOFOLLOW_LINKS).use { input ->
            while (count < buffer.size) {
                val read = input.read(buffer, count, buffer.size - count)
                if (read < 0) break
                require(read > 0); count += read
            }
        }
        require(count <= MARKER_LIMIT)
        return buffer.copyOf(count)
    }
    private fun identity(path: Path): Identity = try {
        val data = Files.readAttributes(path, "unix:dev,ino,nlink,size,lastModifiedTime,ctime,mode,uid", LinkOption.NOFOLLOW_LINKS)
        Identity((data.getValue("dev") as Number).toLong(), (data.getValue("ino") as Number).toLong(),
            (data.getValue("nlink") as Number).toLong(), (data.getValue("size") as Number).toLong(),
            data.getValue("lastModifiedTime").toString(), data.getValue("ctime").toString(),
            (data.getValue("mode") as Number).toInt(), (data.getValue("uid") as Number).toInt())
    } catch (_: UnsupportedOperationException) { androidIdentity(path) }
      catch (_: IllegalArgumentException) { androidIdentity(path) }
    private fun androidIdentity(path: Path): Identity {
        val data = Os.lstat(path.toString())
        return Identity(data.st_dev, data.st_ino, data.st_nlink, data.st_size,
            "${data.st_mtim.tv_sec}:${data.st_mtim.tv_nsec}", "${data.st_ctim.tv_sec}:${data.st_ctim.tv_nsec}", data.st_mode, data.st_uid)
    }
    private fun stageMarker(data: JSONObject?, library: String, id: String): Boolean = data != null &&
        data.keys().asSequence().toSet() == setOf("format", "purpose", "library", "stage", "created_at_millis") &&
        data.opt("format") == 1 && data.optString("purpose") == "restore-stage" && data.optString("library") == library &&
        data.optString("stage") == id && data.opt("created_at_millis") is Number
    private fun retirementMarker(data: JSONObject?, library: String, id: String): Boolean = data != null &&
        data.keys().asSequence().toSet() == setOf("format", "purpose", "library", "id", "original", "disposition", "created_at_millis") &&
        data.opt("format") == 1 && data.optString("purpose") == "restore-retirement" && data.optString("library") == library &&
        data.optString("id") == id && runCatching { UUID.fromString(id).toString() }.getOrNull() == id &&
        validName(data.optString("original")) && data.optString("disposition") in Disposition.values().map { it.name } &&
        data.opt("created_at_millis") is Number
    private fun validName(value: String) = value.isNotEmpty() && value != "." && value != ".." && !value.contains('/') && !value.contains('\u0000')
    private fun digest(bytes: ByteArray) = MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }
    private fun writeNew(file: File, bytes: ByteArray) {
        require(bytes.size <= MARKER_LIMIT)
        FileChannel.open(file.toPath(), setOf(StandardOpenOption.CREATE_NEW, StandardOpenOption.WRITE),
            PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rw-------"))).use { output ->
            val buffer = ByteBuffer.wrap(bytes); while (buffer.hasRemaining()) output.write(buffer); output.force(true)
        }
    }
    private fun privateDirectory(file: File) {
        if (!Files.exists(file.toPath(), LinkOption.NOFOLLOW_LINKS)) Files.createDirectory(file.toPath(),
            PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rwx------")))
        val info = identity(file.toPath()); require(info.directory && info.private)
    }
    private fun sync(file: File) { FileChannel.open(file.toPath(), StandardOpenOption.READ).use { it.force(true) } }
}
