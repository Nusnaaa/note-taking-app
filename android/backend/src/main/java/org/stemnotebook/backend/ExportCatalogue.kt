package org.stemnotebook.backend

import android.content.ContentValues
import android.database.sqlite.SQLiteDatabase
import android.net.Uri
import android.system.Os
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.io.IOException
import java.nio.file.Files
import java.nio.file.LinkOption
import java.nio.file.attribute.PosixFilePermissions
import java.util.Collections
import java.util.UUID

/** Durable attempt metadata only. Never stores content or recovery keys. */
internal class ExportCatalogue(private val directory: File) {
    private data class Identity(val device: Long,val inode: Long,val links: Long,val mode: Int,val uid: Int)
    private var anchor: Identity? = null
    private val file get() = File(directory,"catalogue.db")
    enum class State { Uncertain, ProviderVerified }
    data class Part(val name: String,val bytes: Long,val sha256: String,val document: Uri? = null)
    data class Record(val id: UUID,val library: UUID,val format: String,val createdMillis: Long,val tree: Uri,val folder: Uri?,
        val parts: List<Part>,val state: State,val descriptorSynced: Boolean)
    @Synchronized fun write(record: Record) {
        validate(record)
        database(true) { db ->
            val old = read(db,record.id)
            if(old != null) {
                require(old.library==record.library && old.format==record.format && old.createdMillis==record.createdMillis && old.tree==record.tree)
                require(old.parts.map { it.copy(document=null) }==record.parts.map { it.copy(document=null) })
                require(old.folder==null || old.folder==record.folder)
                old.parts.zip(record.parts).forEach { (a,b) -> require(a.document==null || a.document==b.document) }
                require(old.state!=State.ProviderVerified || old==record)
            } else require(record.state==State.Uncertain)
            val values=ContentValues().apply { put("id",record.id.toString());put("payload",encode(record).toString()) }
            if(old==null) require(db.insertOrThrow("publications",null,values)>0)
            else require(db.update("publications",values,"id=?",arrayOf(record.id.toString()))==1)
        }
    }
    @Synchronized fun lookup(id: UUID,library: UUID): Record? = database(false) { db -> read(db,id)?.also { require(it.library==library) } }
    private fun read(db: SQLiteDatabase,id: UUID): Record? = db.rawQuery("SELECT payload FROM publications WHERE id=?",arrayOf(id.toString())).use { c ->
        if(!c.moveToFirst()) null else {
            require(c.getType(0)==android.database.Cursor.FIELD_TYPE_STRING)
            val text=c.getString(0);require(text.toByteArray(Charsets.UTF_8).size<=8*1024*1024)
            decode(JSONObject(text)).also { require(it.id==id && !c.moveToNext()) }
        }
    }
    private fun validate(r: Record) {
        require(r.format in setOf("Native","Markdown","Pdf") && r.createdMillis>=0 && r.parts.isNotEmpty())
        val names=r.parts.map { it.name }; require(names.toSet().size==names.size)
        uri(r.tree)
        r.folder?.let { uri(it);require(it.authority==r.tree.authority && it!=r.tree) }
        r.parts.forEach { p ->
            require(p.name.matches(Regex("[A-Za-z0-9][A-Za-z0-9._-]{0,159}")) && p.bytes>=0 && p.sha256.matches(Regex("[0-9a-f]{64}")))
            p.document?.let { uri(it);require(r.folder!=null && it!=r.folder && it.authority==r.tree.authority) }
        }
        require(r.parts.mapNotNull { it.document }.toSet().size==r.parts.count { it.document!=null })
        if(r.state==State.ProviderVerified) require(r.folder!=null && r.parts.all { it.document!=null })
        require(encode(r).toString().toByteArray(Charsets.UTF_8).size<=8*1024*1024)
    }
    private fun uri(value: Uri) { require(value.scheme=="content" && !value.authority.isNullOrEmpty() && value.toString().length<=4096) }
    private fun encode(r: Record)=JSONObject().put("format_version",1).put("id",r.id.toString()).put("library",r.library.toString())
        .put("format",r.format).put("created_ms",r.createdMillis).put("tree",r.tree.toString()).put("folder",r.folder?.toString() ?: JSONObject.NULL)
        .put("state",r.state.name).put("synced",r.descriptorSynced).put("parts",JSONArray(r.parts.map { p -> JSONObject()
            .put("name",p.name).put("bytes",p.bytes).put("sha256",p.sha256).put("document",p.document?.toString() ?: JSONObject.NULL) }))
    private fun decode(j: JSONObject): Record {
        require(j.getInt("format_version")==1)
        val parts=j.getJSONArray("parts")
        val r=Record(UUID.fromString(j.getString("id")),UUID.fromString(j.getString("library")),j.getString("format"),j.getLong("created_ms"),Uri.parse(j.getString("tree")),
            if(j.isNull("folder")) null else Uri.parse(j.getString("folder")),Collections.unmodifiableList((0 until parts.length()).map { i ->
                val p=parts.getJSONObject(i);Part(p.getString("name"),p.getLong("bytes"),p.getString("sha256"),if(p.isNull("document")) null else Uri.parse(p.getString("document"))) }),
            State.valueOf(j.getString("state")),j.getBoolean("synced"))
        validate(r);return r
    }
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
                    db.execSQL("INSERT INTO metadata VALUES('purpose','STEM-EXPORT-CATALOGUE-1')")
                    db.execSQL("CREATE TABLE publications(sequence INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT NOT NULL UNIQUE,payload TEXT NOT NULL)")
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
        db.rawQuery("SELECT name,type FROM sqlite_master WHERE type IN ('table','view','trigger') AND name NOT LIKE 'sqlite_%'",null).use { c ->
            val names = mutableSetOf<String>()
            while(c.moveToNext()) { require(c.getString(1)=="table"); names.add(c.getString(0)) }
            require(names==setOf("metadata","publications"))
        }
        db.rawQuery("SELECT key,value FROM metadata",null).use { c ->
            require(c.moveToFirst() && c.getString(0)=="purpose" && c.getString(1)=="STEM-EXPORT-CATALOGUE-1" && !c.moveToNext())
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
