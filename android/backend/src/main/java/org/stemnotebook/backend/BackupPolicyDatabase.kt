package org.stemnotebook.backend

import android.database.sqlite.SQLiteDatabase
import android.system.Os
import java.io.File
import java.util.UUID
import java.nio.file.Files
import java.nio.file.LinkOption
import java.nio.file.attribute.PosixFilePermissions

/** Owns only device-local backup policy SQLite connections. Unknown files are
 * never adopted or repaired; only the exact known v1 format is migrated. Callers serialize complete transactions. */
internal class BackupPolicyDatabase(private val directory: File) {
    private data class Identity(val device: Long, val inode: Long, val links: Long, val mode: Int, val uid: Int)
    private var anchor: Identity? = null
    private val file get() = File(directory, "policy.db")
    @Synchronized fun <T> access(write: Boolean, action: (SQLiteDatabase) -> T): T {
        secureDirectory(); checkFiles()
        val new = try {
            Files.createFile(file.toPath(), PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rw-------"))); true
        } catch (_: java.nio.file.FileAlreadyExistsException) { false }
        checkFiles(); val identity = stat(file)
        val flags = SQLiteDatabase.NO_LOCALIZED_COLLATORS
        if (!new) {
            SQLiteDatabase.openDatabase(file.path, null, flags or SQLiteDatabase.OPEN_READONLY).use {
                validate(it)
                if (!write && it.version == 2) return action(it)
            }
            checkFiles(); require(stat(file) == identity)
        }
        val result = SQLiteDatabase.openDatabase(file.path, null, flags or SQLiteDatabase.OPEN_READWRITE).use { db ->
            if (new) {
                db.execSQL("PRAGMA synchronous=FULL")
                db.beginTransaction()
                try {
                    schema.forEach { (name, columns) -> db.execSQL("CREATE TABLE $name($columns)") }
                    db.execSQL("INSERT INTO metadata VALUES('purpose','STEM-BACKUP-POLICY-1')")
                    db.version = 2; db.setTransactionSuccessful()
                } finally { db.endTransaction() }
            }
            validate(db)
            if (db.version == 1) {
                db.execSQL("PRAGMA synchronous=FULL")
                db.beginTransaction()
                try {
                    // Legacy configuration did not record a consented size cap.
                    // Keep all credential/consent/run records; require renewed
                    // scheduling review instead of guessing a cap or enabling.
                    db.execSQL("CREATE TABLE schedule_limits(${schema.getValue("schedule_limits")})")
                    var after = ""
                    while (true) {
                        val id = db.rawQuery("SELECT id FROM configurations WHERE id>? ORDER BY id LIMIT 1", arrayOf(after)).use { c ->
                            if (!c.moveToFirst()) null else c.getString(0)
                        } ?: break
                        require(UUID.fromString(id).toString() == id)
                        db.execSQL("UPDATE configurations SET state='Disabled',revision=? WHERE id=?", arrayOf(UUID.randomUUID().toString(), id))
                        after = id
                    }
                    db.version = 2; db.setTransactionSuccessful()
                } finally { db.endTransaction() }
                validate(db)
            }
            if (write) db.execSQL("PRAGMA synchronous=FULL")
            action(db)
        }
        java.nio.channels.FileChannel.open(directory.toPath(), java.nio.file.StandardOpenOption.READ).use { it.force(true) }
        if (new) java.nio.channels.FileChannel.open(directory.parentFile!!.toPath(), java.nio.file.StandardOpenOption.READ).use { it.force(true) }
        return result
    }
    private val schema = linkedMapOf(
        "metadata" to "key TEXT PRIMARY KEY,value TEXT NOT NULL",
        "credentials" to "id TEXT PRIMARY KEY,library TEXT NOT NULL,purpose TEXT NOT NULL,backup TEXT,state TEXT NOT NULL,consented_ms INTEGER NOT NULL,envelope BLOB",
        "configurations" to "id TEXT PRIMARY KEY,library TEXT NOT NULL UNIQUE,revision TEXT NOT NULL,state TEXT NOT NULL,kind TEXT NOT NULL,destination TEXT NOT NULL,credential TEXT NOT NULL,permission TEXT,interval_ms INTEGER NOT NULL,original_ink INTEGER NOT NULL,source_assets INTEGER NOT NULL,consented_ms INTEGER NOT NULL,active_run TEXT",
        "runs" to "id TEXT PRIMARY KEY,schedule TEXT NOT NULL,revision TEXT NOT NULL,started_ms INTEGER NOT NULL,finished_ms INTEGER,result TEXT,publication_sequence INTEGER",
        "schedule_limits" to "schedule TEXT PRIMARY KEY,revision TEXT NOT NULL,maximum_bytes INTEGER NOT NULL,destination_identity TEXT NOT NULL",
        "permissions" to "id TEXT PRIMARY KEY,library TEXT NOT NULL,tree TEXT NOT NULL,identity TEXT NOT NULL,previous_read INTEGER NOT NULL,previous_write INTEGER NOT NULL,consented_ms INTEGER NOT NULL,outcome TEXT,observed_read INTEGER,observed_write INTEGER",
    )
    private fun validate(db: SQLiteDatabase) {
        require(db.version == 1 || db.version == 2)
        db.rawQuery("SELECT key,value FROM metadata", null).use { c ->
            require(c.moveToFirst() && c.getString(0) == "purpose" && c.getString(1) == "STEM-BACKUP-POLICY-1" && !c.moveToNext())
        }
        val expected = if (db.version == 1) schema.filterKeys { it != "schedule_limits" } else schema
        db.rawQuery("SELECT type,name,sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' LIMIT 8", null).use { c ->
            val found = mutableSetOf<String>()
            while (c.moveToNext()) {
                val name = c.getString(1)
                require(c.getString(0) == "table" && expected[name]?.let { c.getString(2) == "CREATE TABLE $name($it)" } == true)
                require(found.add(name))
            }
            require(found == expected.keys)
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
                require(entry.fileName.toString() in setOf("policy.db", "policy.db-journal", "policy.db-wal", "policy.db-shm"))
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
