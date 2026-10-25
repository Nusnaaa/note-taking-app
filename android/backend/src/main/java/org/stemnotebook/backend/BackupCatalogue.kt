package org.stemnotebook.backend

import android.content.ContentValues
import android.database.Cursor
import android.database.sqlite.SQLiteDatabase
import android.net.Uri
import android.system.Os
import java.io.File
import java.io.IOException
import java.nio.file.Files
import java.nio.file.LinkOption
import java.nio.file.attribute.PosixFilePermissions
import java.util.UUID

/** Publication history, never a promise of future provider/file availability. */
data class BackupPublicationRecord(
    val sequence: Long, val id: String, val library: String, val generation: String,
    val acknowledgedAtMillis: Long, val destinationKind: BackupDestinationKind,
    val destination: String, val bytes: Long, val ciphertextSha256: String,
    val acknowledgement: BackupAcknowledgement, val descriptorSyncAcknowledged: Boolean,
    val includesOriginalInk: Boolean, val sourceAssets: Long, val omissions: List<String>,
)
enum class BackupDestinationKind { LocalFile, ProviderDocument }
enum class BackupAcknowledgement { LocalDurable, ProviderReadBack }
internal data class BackupSnapshot(val library: String, val generation: String, val authoringQuiescent: Boolean)
/** These are past acknowledgements. No destination is contacted by this query. */
data class BackupStatus(
    val lastPublication: BackupPublicationRecord?,
    val currentCommittedSnapshotPublication: BackupPublicationRecord?,
    val authoringQuiescent: Boolean,
) {
    val currentAuthoringWasPublished: Boolean get() = authoringQuiescent && currentCommittedSnapshotPublication != null
}

/** Device-private metadata, excluded from notebook backups. Its caller owns the
 * app library lease and invokes this only on the optional backup worker. No
 * content, credentials or native notebook database connection is held here. */
internal class BackupCatalogue(private val directory: File, private val fault: (String) -> Unit = {}) {
    private data class Identity(val device: Long, val inode: Long, val links: Long, val mode: Int, val uid: Int)
    private var anchor: Identity? = null
    private val file get() = File(directory, "catalogue.db")
    class Unavailable : IOException("Backup publication history unavailable")

    @Synchronized fun append(snapshot: BackupSnapshot, kind: BackupDestinationKind, location: String,
        bytes: Long, digest: String, acknowledgement: BackupAcknowledgement, descriptorSynced: Boolean,
        includesOriginalInk: Boolean = false, sourceAssets: Long = 0, omissions: List<String> = emptyList(), publicationId: String = UUID.randomUUID().toString()): BackupPublicationRecord = guarded {
        validateSnapshot(snapshot)
        requireUuid(publicationId)
        validateLocation(kind, location)
        require(bytes > 0 && digest.matches(Regex("[0-9a-f]{64}")))
        require((kind == BackupDestinationKind.LocalFile) == (acknowledgement == BackupAcknowledgement.LocalDurable))
        require(kind != BackupDestinationKind.LocalFile || descriptorSynced)
        validateInclusions(sourceAssets, omissions)
        database(write = true) { db ->
            val id = publicationId
            val now = System.currentTimeMillis().coerceAtLeast(0)
            val values = ContentValues().apply {
                put("id", id); put("library", snapshot.library); put("generation", snapshot.generation)
                put("acknowledged_ms", now); put("kind", kind.name); put("destination", location)
                put("bytes", bytes); put("sha256", digest); put("acknowledgement", acknowledgement.name)
                put("descriptor_synced", if (descriptorSynced) 1 else 0)
                put("original_ink", if (includesOriginalInk) 1 else 0); put("source_assets", sourceAssets)
                put("omissions", org.json.JSONArray(omissions).toString())
            }
            db.beginTransaction()
            val sequence = try {
                val inserted = db.insertOrThrow("publications", null, values)
                fault("catalogue-before-commit")
                db.setTransactionSuccessful()
                inserted
            } finally { db.endTransaction() }
            fault("catalogue-after-commit")
            BackupPublicationRecord(sequence, id, snapshot.library, snapshot.generation, now, kind, location,
                bytes, digest, acknowledgement, descriptorSynced, includesOriginalInk, sourceAssets, omissions.toList())
        }
    }

    @Synchronized fun status(snapshot: BackupSnapshot): BackupStatus = guarded {
        validateSnapshot(snapshot)
        database(write = false) { db ->
            val last = query(db, "WHERE library=? ORDER BY sequence DESC LIMIT 1", arrayOf(snapshot.library)).singleOrNull()
            val matching = query(db, "WHERE library=? AND generation=? ORDER BY sequence DESC LIMIT 1", arrayOf(snapshot.library, snapshot.generation)).singleOrNull()
            BackupStatus(last, matching, snapshot.authoringQuiescent)
        }
    }

    @Synchronized fun page(library: String, before: Long?, limit: Int): List<BackupPublicationRecord> = guarded {
        requireUuid(library); require(limit in 1..100 && (before == null || before > 0))
        database(write = false) { db ->
            if (before == null) query(db, "WHERE library=? ORDER BY sequence DESC LIMIT ?", arrayOf(library, limit.toString()))
            else query(db, "WHERE library=? AND sequence<? ORDER BY sequence DESC LIMIT ?", arrayOf(library, before.toString(), limit.toString()))
        }
    }

    private fun query(db: SQLiteDatabase, suffix: String, args: Array<String>): List<BackupPublicationRecord> {
        val rows = mutableListOf<BackupPublicationRecord>()
        db.rawQuery("SELECT sequence,id,library,generation,acknowledged_ms,kind,destination,bytes,sha256,acknowledgement,descriptor_synced,original_ink,source_assets,omissions FROM publications $suffix", args).use { cursor ->
            while (cursor.moveToNext()) rows.add(decode(cursor))
        }
        return rows
    }
    private fun decode(row: Cursor): BackupPublicationRecord {
        for (column in listOf(0, 4, 7, 10, 11, 12)) require(row.getType(column) == Cursor.FIELD_TYPE_INTEGER)
        for (column in listOf(1, 2, 3, 5, 6, 8, 9, 13)) require(row.getType(column) == Cursor.FIELD_TYPE_STRING)
        val snapshot = BackupSnapshot(row.getString(2), row.getString(3), false)
        validateSnapshot(snapshot); requireUuid(row.getString(1))
        val kind = BackupDestinationKind.valueOf(row.getString(5))
        val acknowledgement = BackupAcknowledgement.valueOf(row.getString(9))
        validateLocation(kind, row.getString(6))
        require(row.getLong(0) > 0 && row.getLong(4) >= 0 && row.getLong(7) > 0)
        require(row.getString(8).matches(Regex("[0-9a-f]{64}")))
        val synced = row.getInt(10); require(synced == 0 || synced == 1)
        require((kind == BackupDestinationKind.LocalFile) == (acknowledgement == BackupAcknowledgement.LocalDurable))
        require(kind != BackupDestinationKind.LocalFile || synced == 1)
        val ink = row.getInt(11); require(ink == 0 || ink == 1)
        val omissionsJson = row.getString(13); require(omissionsJson.length <= 196610)
        val array = org.json.JSONArray(omissionsJson); require(array.length() <= 8)
        val omissions = (0 until array.length()).map { require(array.get(it) is String); array.getString(it) }
        val sourceAssets = row.getLong(12); validateInclusions(sourceAssets, omissions)
        return BackupPublicationRecord(row.getLong(0), row.getString(1), snapshot.library, snapshot.generation,
            row.getLong(4), kind, row.getString(6), row.getLong(7), row.getString(8), acknowledgement, synced == 1, ink == 1, sourceAssets, omissions)
    }
    private fun validateInclusions(sourceAssets: Long, omissions: List<String>) {
        require(sourceAssets >= 0 && omissions.size <= 8)
        require(omissions.all { it.isNotEmpty() && it.toByteArray(Charsets.UTF_8).size <= 4096 })
    }
    private fun validateSnapshot(snapshot: BackupSnapshot) {
        requireUuid(snapshot.library)
        require(snapshot.generation.length in 1..32 && snapshot.generation.toLongOrNull()?.let { it >= 0 } == true)
    }
    private fun validateLocation(kind: BackupDestinationKind, location: String) {
        require(location.length in 1..4096 && !location.contains('\u0000'))
        when (kind) {
            BackupDestinationKind.LocalFile -> require(File(location).isAbsolute)
            BackupDestinationKind.ProviderDocument -> require(Uri.parse(location).let { it.scheme == "content" && !it.authority.isNullOrEmpty() })
        }
    }
    private fun requireUuid(value: String) { require(value.length == 36 && UUID.fromString(value).toString() == value) }
    private fun <T> guarded(action: () -> T): T = try { action() } catch (_: Exception) { throw Unavailable() }

    private fun <T> database(write: Boolean, action: (SQLiteDatabase) -> T): T {
        secureDirectory()
        checkFiles() // Classify the namespace before creating even an empty DB.
        val new = try {
            Files.createFile(file.toPath(), PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rw-------")))
            true
        } catch (_: java.nio.file.FileAlreadyExistsException) { false }
        checkFiles()
        val identity = stat(file)
        val flags = SQLiteDatabase.NO_LOCALIZED_COLLATORS
        if (!new) {
            // Never initialise or migrate an unknown pre-existing SQLite file.
            SQLiteDatabase.openDatabase(file.path, null, flags or SQLiteDatabase.OPEN_READONLY).use {
                validateDatabase(it)
                if (!write) return action(it)
            }
            checkFiles(); require(stat(file) == identity)
        }
        val result = SQLiteDatabase.openDatabase(file.path, null, flags or SQLiteDatabase.OPEN_READWRITE).use { db ->
            if (new) {
                db.execSQL("PRAGMA synchronous=FULL")
                db.beginTransaction()
                try {
                    db.execSQL("CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL)")
                    db.execSQL("INSERT INTO metadata VALUES('purpose','STEM-BACKUP-CATALOGUE-1')")
                    db.execSQL("CREATE TABLE publications(sequence INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT NOT NULL UNIQUE,library TEXT NOT NULL,generation TEXT NOT NULL,acknowledged_ms INTEGER NOT NULL,kind TEXT NOT NULL,destination TEXT NOT NULL,bytes INTEGER NOT NULL,sha256 TEXT NOT NULL,acknowledgement TEXT NOT NULL,descriptor_synced INTEGER NOT NULL,original_ink INTEGER NOT NULL,source_assets INTEGER NOT NULL,omissions TEXT NOT NULL)")
                    db.execSQL("CREATE INDEX publications_library ON publications(library,sequence)")
                    db.execSQL("CREATE INDEX publications_snapshot ON publications(library,generation,sequence)")
                    db.version = 1; db.setTransactionSuccessful()
                } finally { db.endTransaction() }
            }
            validateDatabase(db)
            if (write) db.execSQL("PRAGMA synchronous=FULL")
            action(db)
        }
        // SQLite owns its file descriptors. Sync only directory descriptors here,
        // after the metadata connection closes; never raw-open a database file.
        syncDirectory(directory)
        if (new) syncDirectory(checkNotNull(directory.parentFile))
        return result
    }
    private fun syncDirectory(directory: File) {
        java.nio.channels.FileChannel.open(directory.toPath(), java.nio.file.StandardOpenOption.READ).use { it.force(true) }
    }
    private fun validateDatabase(db: SQLiteDatabase) {
        require(db.version == 1)
        db.rawQuery("SELECT key,value FROM metadata", null).use { c ->
            require(c.moveToFirst() && c.getString(0) == "purpose" && c.getString(1) == "STEM-BACKUP-CATALOGUE-1" && !c.moveToNext())
        }
        db.rawQuery("SELECT name FROM sqlite_master WHERE type IN ('table','view','trigger') AND name NOT LIKE 'sqlite_%' LIMIT 3", null).use { c ->
            val names = mutableSetOf<String>(); while (c.moveToNext()) names.add(c.getString(0))
            require(names == setOf("metadata", "publications"))
        }
        for ((name, columns) in listOf("publications_library" to listOf("library", "sequence"), "publications_snapshot" to listOf("library", "generation", "sequence"))) {
            db.rawQuery("PRAGMA index_info($name)", null).use { c ->
                val actual = mutableListOf<String>()
                while (c.moveToNext()) { require(actual.size < 4); actual.add(c.getString(2)) }
                require(actual == columns)
            }
        }
        db.rawQuery("PRAGMA table_info(publications)", null).use { c ->
            val names = mutableListOf<String>(); while (c.moveToNext()) names.add(c.getString(1))
            require(names == listOf("sequence","id","library","generation","acknowledged_ms","kind","destination","bytes","sha256","acknowledgement","descriptor_synced","original_ink","source_assets","omissions"))
        }
    }
    private fun secureDirectory() {
        val parent = checkNotNull(directory.parentFile)
        require(directory.isAbsolute && !Files.isSymbolicLink(parent.toPath()))
        // App Context paths may contain trusted ancestor aliases (/data/user/0).
        // Check the actual leaf identity, and never recreate an already anchored
        // directory after namespace replacement.
        if (anchor == null) {
            try { Files.createDirectory(directory.toPath(), PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rwx------"))) }
            catch (_: java.nio.file.FileAlreadyExistsException) { }
        }
        val current = stat(directory)
        require(current.mode and 0xf000 == 0x4000 && current.mode and 0x3f == 0)
        anchor?.let { require(it == current) } ?: run { anchor = current }
    }
    private fun checkFiles() {
        secureDirectory()
        Files.newDirectoryStream(directory.toPath()).use { entries ->
            for (entry in entries) {
                require(entry.fileName.toString() in setOf("catalogue.db", "catalogue.db-journal", "catalogue.db-wal", "catalogue.db-shm"))
                val data = stat(entry.toFile())
                require(data.mode and 0xf000 == 0x8000 && data.mode and 0x3f == 0 && data.links == 1L && data.uid == anchor!!.uid)
            }
        }
    }
    private fun stat(file: File): Identity = try {
        val a = Files.readAttributes(file.toPath(), "unix:dev,ino,nlink,mode,uid", LinkOption.NOFOLLOW_LINKS)
        Identity((a.getValue("dev") as Number).toLong(), (a.getValue("ino") as Number).toLong(),
            (a.getValue("nlink") as Number).toLong(), (a.getValue("mode") as Number).toInt(), (a.getValue("uid") as Number).toInt())
    } catch (_: UnsupportedOperationException) { androidStat(file) }
      catch (_: IllegalArgumentException) { androidStat(file) }
    private fun androidStat(file: File): Identity = Os.lstat(file.path).let { Identity(it.st_dev, it.st_ino, it.st_nlink, it.st_mode, it.st_uid) }
}
