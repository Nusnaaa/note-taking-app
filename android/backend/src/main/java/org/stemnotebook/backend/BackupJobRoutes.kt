package org.stemnotebook.backend

import android.content.Context
import android.database.Cursor
import android.database.sqlite.SQLiteDatabase
import android.system.Os
import java.io.File
import java.io.IOException
import java.nio.file.Files
import java.nio.file.LinkOption
import java.nio.file.attribute.PosixFilePermissions
import java.util.UUID

/** Device-private routing only. No notebook data, credential, URI permission or
 * enabled policy is created by constructing this object or reading an absent map. */
internal class BackupJobRoutes(private val primary: File, private val fault: (String) -> Unit = {}) {
    class Unavailable : IOException("Backup job routing unavailable")
    data class Root(val path: String, val device: String, val inode: String, val uid: Int) {
        fun file(): File = File(path).also { require(inspect(it) == this) }
        companion object {
            fun inspect(folder: File): Root {
                val canonical = folder.canonicalFile
                require(canonical.isAbsolute && canonical.path.length in 1..4096)
                val attributes = stat(canonical)
                require(attributes.mode and 0xf000 == 0x4000)
                return Root(canonical.path, attributes.device, attributes.inode, attributes.uid)
            }
        }
    }
    data class Route(val job: Int, val root: Root, val library: UUID, val token: BackupJobScheduler.Token)
    private val directory = File(primary.canonicalFile, "backup-job-routes")
    private val file = File(directory, "routes.db")
    private var anchor: Identity? = null
    private val columns = "job,root,device,inode,uid,library,schedule,revision"
    private fun bounded(column: String, limit: Int) = "CASE WHEN length($column)<=$limit THEN $column ELSE NULL END"
    private val readColumns = "job,${bounded("root", 4096)},${bounded("device", 32)},${bounded("inode", 32)},uid," +
        "${bounded("library", 36)},${bounded("schedule", 36)},${bounded("revision", 36)}"
    private val schema = linkedMapOf(
        "metadata" to "key TEXT PRIMARY KEY,value TEXT NOT NULL",
        "routes" to "job INTEGER PRIMARY KEY,root TEXT NOT NULL UNIQUE,device TEXT NOT NULL,inode TEXT NOT NULL,uid INTEGER NOT NULL,library TEXT NOT NULL UNIQUE,schedule TEXT NOT NULL,revision TEXT NOT NULL",
    )

    /** Persist before OS registration. A rejected/lost OS response never invents
     * Enabled policy; a later callback must still pass the root's live policy. */
    fun reserve(root: Root, library: UUID, token: BackupJobScheduler.Token, available: (Int) -> Boolean): Route = guarded {
        require(root.file().canonicalPath == root.path && root.uid == Root.inspect(primary).uid)
        checkNotNull(access(true) { db ->
            db.beginTransaction()
            try {
                val old = byRoot(db, root.path)
                val primaryRoot = root.path == primary.canonicalPath
                val slot = if (old != null) {
                    require(old.root == root && old.library == library && old.token.schedule == token.schedule)
                    require(available(old.job)); old.job
                } else if (primaryRoot) {
                    require(byJob(db, BackupJobScheduler.JOB_ID) == null && available(BackupJobScheduler.JOB_ID))
                    BackupJobScheduler.JOB_ID
                } else {
                    // Finite reserved OS identifier namespace: an admission cap,
                    // not a performance promise or a silently evicted library.
                    (1 until BackupJobScheduler.JOB_SLOTS).map { BackupJobScheduler.JOB_ID + it }
                        .firstOrNull { byJob(db, it) == null && available(it) } ?: throw Unavailable()
                }
                val route = Route(slot, root, library, token)
                if (old == null) db.execSQL("INSERT INTO routes($columns) VALUES(?,?,?,?,?,?,?,?)", arrayOf(slot, root.path,
                    root.device, root.inode, root.uid, library.toString(), token.schedule.toString(), token.revision.toString()))
                else {
                    val values = android.content.ContentValues().apply { put("revision", token.revision.toString()) }
                    require(db.update("routes", values, "job=? AND schedule=? AND revision=?",
                        arrayOf(slot.toString(), old.token.schedule.toString(), old.token.revision.toString())) == 1)
                }
                db.setTransactionSuccessful(); route
            } finally { db.endTransaction() }
        })
    }
    /** Called only after reviewed native disablement and actual run retirement.
     * The callback observes/cancels OS registration while holding the same map
     * lock used by registration. A lost commit/OS acknowledgement is uncertain. */
    fun retire(root: Root, library: UUID, schedule: UUID, stillDisabled: () -> Unit,
        registration: (Route?) -> BackupJobScheduler.Retirement): BackupJobScheduler.Retirement = try {
        synchronized(databaseLock) {
            stillDisabled()
            access(false, true) { db ->
                db.beginTransaction()
                try {
                    val prior = byRoot(db, root.path)
                    val result = if (prior == null) registration(null)
                    else if (prior.root != root || prior.library != library || prior.token.schedule != schedule) BackupJobScheduler.Retirement.Conflict
                    else {
                        root.file(); stillDisabled()
                        when (val observed = registration(prior)) {
                            BackupJobScheduler.Retirement.Released, BackupJobScheduler.Retirement.Absent -> {
                                stillDisabled()
                                require(db.delete("routes", "job=? AND root=? AND library=? AND schedule=? AND revision=?",
                                    arrayOf(prior.job.toString(), root.path, library.toString(), schedule.toString(), prior.token.revision.toString())) == 1)
                                BackupJobScheduler.Retirement.Released
                            }
                            else -> observed
                        }
                    }
                    db.setTransactionSuccessful(); result
                } finally { db.endTransaction() }
            } ?: registration(null)
        }
    } catch (_: Exception) { BackupJobScheduler.Retirement.Uncertain }
    fun root(root: File): Route? = guarded { access(false) { byRoot(it, root.canonicalPath) } }
    fun job(job: Int): Route? = guarded { access(false) { byJob(it, job) } }
    data class Resolved(val root: Root, val library: UUID?)
    fun resolve(job: Int, token: BackupJobScheduler.Token): Resolved = guarded {
        require(BackupJobScheduler.ownsId(job))
        val route = this.job(job)
        if (route != null) { require(route.token == token); route.root.file(); Resolved(route.root, route.library) }
        else {
            // Existing primary-root jobs predate the routing map. Other reserved
            // slots never fall back to the primary or create a missing library.
            require(job == BackupJobScheduler.JOB_ID)
            Resolved(Root.inspect(primary), null)
        }
    }
    private fun byRoot(db: SQLiteDatabase, root: String): Route? =
        db.rawQuery("SELECT $readColumns FROM routes WHERE root=? LIMIT 2", arrayOf(root)).use(::one)
    private fun byJob(db: SQLiteDatabase, job: Int): Route? =
        db.rawQuery("SELECT $readColumns FROM routes WHERE job=? LIMIT 2", arrayOf(job.toString())).use(::one)
    private fun one(cursor: Cursor): Route? {
        if (!cursor.moveToFirst()) return null
        require(cursor.getType(0) == Cursor.FIELD_TYPE_INTEGER && cursor.getType(4) == Cursor.FIELD_TYPE_INTEGER)
        for (i in listOf(1, 2, 3, 5, 6, 7)) require(cursor.getType(i) == Cursor.FIELD_TYPE_STRING)
        val jobValue = cursor.getLong(0); val uidValue = cursor.getLong(4)
        require(jobValue >= BackupJobScheduler.JOB_ID.toLong() && jobValue < BackupJobScheduler.JOB_ID.toLong() + BackupJobScheduler.JOB_SLOTS)
        require(uidValue in 0L..Int.MAX_VALUE.toLong())
        val job = jobValue.toInt()
        val root = Root(cursor.getString(1), cursor.getString(2), cursor.getString(3), uidValue.toInt())
        require(File(root.path).isAbsolute && root.path.length in 1..4096 && !root.path.contains('\u0000'))
        require(root.device.toLong() >= 0 && root.inode.toLong() > 0 && root.uid >= 0)
        fun uuid(index: Int): UUID = cursor.getString(index).let { text ->
            require(text.length == 36); UUID.fromString(text).also { require(it.toString() == text) }
        }
        return Route(job, root, uuid(5), BackupJobScheduler.Token(uuid(6), uuid(7))).also { require(!cursor.moveToNext()) }
    }
    private fun <T> access(create: Boolean, write: Boolean = create, action: (SQLiteDatabase) -> T): T? = synchronized(databaseLock) { accessUnlocked(create, write, action) }
    private fun <T> accessUnlocked(create: Boolean, write: Boolean, action: (SQLiteDatabase) -> T): T? {
        if (!Files.exists(directory.toPath(), LinkOption.NOFOLLOW_LINKS)) {
            require(anchor == null)
            if (!create) return null
            try { Files.createDirectory(directory.toPath(), PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rwx------"))) }
            catch (_: java.nio.file.FileAlreadyExistsException) { }
        }
        checkFiles()
        val new = if (create) try {
            Files.createFile(file.toPath(), PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rw-------"))); true
        } catch (_: java.nio.file.FileAlreadyExistsException) { false }
        else { if (!Files.exists(file.toPath(), LinkOption.NOFOLLOW_LINKS)) return null; false }
        checkFiles(); val identity = stat(file)
        val flags = SQLiteDatabase.NO_LOCALIZED_COLLATORS
        if (!new) {
            SQLiteDatabase.openDatabase(file.path, null, flags or SQLiteDatabase.OPEN_READONLY).use { db ->
                validate(db)
                if (!write) return action(db)
            }
            checkFiles(); require(stat(file) == identity)
        }
        val result = SQLiteDatabase.openDatabase(file.path, null, flags or SQLiteDatabase.OPEN_READWRITE).use { db ->
            db.execSQL("PRAGMA synchronous=FULL")
            if (new) {
                db.beginTransaction()
                try {
                    schema.forEach { (name, columns) -> db.execSQL("CREATE TABLE $name($columns)") }
                    db.execSQL("INSERT INTO metadata VALUES('purpose','STEM-BACKUP-JOB-ROUTES-1')")
                    db.version = 1; db.setTransactionSuccessful()
                } finally { db.endTransaction() }
            }
            validate(db); action(db)
        }
        fault("after-route-commit")
        java.nio.channels.FileChannel.open(directory.toPath(), java.nio.file.StandardOpenOption.READ).use { it.force(true) }
        if (new) java.nio.channels.FileChannel.open(directory.parentFile!!.toPath(), java.nio.file.StandardOpenOption.READ).use { it.force(true) }
        return result
    }
    private fun validate(db: SQLiteDatabase) {
        require(db.version == 1)
        db.rawQuery("SELECT key,value FROM metadata", null).use { c ->
            require(c.moveToFirst() && c.getString(0) == "purpose" && c.getString(1) == "STEM-BACKUP-JOB-ROUTES-1" && !c.moveToNext())
        }
        db.rawQuery("SELECT type,name,sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' LIMIT 3", null).use { c ->
            val found = mutableSetOf<String>()
            while (c.moveToNext()) {
                val name = c.getString(1)
                require(c.getString(0) == "table" && schema[name]?.let { c.getString(2) == "CREATE TABLE $name($it)" } == true && found.add(name))
            }
            require(found == schema.keys)
        }
    }
    private fun checkFiles() {
        val current = stat(directory)
        require(current.mode and 0xf000 == 0x4000 && current.mode and 0x3f == 0)
        anchor?.let { require(it == current) } ?: run { anchor = current }
        Files.newDirectoryStream(directory.toPath()).use { entries ->
            for (entry in entries) {
                require(entry.fileName.toString() in setOf("routes.db", "routes.db-journal", "routes.db-wal", "routes.db-shm"))
                val data = stat(entry.toFile())
                require(data.mode and 0xf000 == 0x8000 && data.mode and 0x3f == 0 && data.links == 1L && data.uid == current.uid)
            }
        }
    }
    private fun <T> guarded(action: () -> T): T = try { action() } catch (_: Exception) { throw Unavailable() }
    companion object {
        private val databaseLock = Any() // Serialize first initialization across root controllers.
        private data class Identity(val device: String, val inode: String, val uid: Int, val mode: Int, val links: Long)
        private fun stat(file: File): Identity = try {
            val a = Files.readAttributes(file.toPath(), "unix:dev,ino,uid,mode,nlink", LinkOption.NOFOLLOW_LINKS)
            Identity(a.getValue("dev").toString(), a.getValue("ino").toString(), (a.getValue("uid") as Number).toInt(),
                (a.getValue("mode") as Number).toInt(), (a.getValue("nlink") as Number).toLong())
        } catch (_: UnsupportedOperationException) { androidStat(file) }
          catch (_: IllegalArgumentException) { androidStat(file) }
        private fun androidStat(file: File): Identity = Os.lstat(file.path).let { Identity(it.st_dev.toString(), it.st_ino.toString(), it.st_uid, it.st_mode, it.st_nlink) }
        fun forApplication(context: Context): BackupJobRoutes = BackupJobRoutes(context.noBackupFilesDir)
    }
}
