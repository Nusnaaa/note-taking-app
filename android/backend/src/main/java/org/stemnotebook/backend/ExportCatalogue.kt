package org.stemnotebook.backend

import android.content.ContentValues
import android.database.Cursor
import android.database.sqlite.SQLiteDatabase
import android.net.Uri
import android.system.Os
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.nio.ByteBuffer
import java.nio.charset.CodingErrorAction
import java.nio.file.Files
import java.nio.file.LinkOption
import java.nio.file.attribute.PosixFilePermissions
import java.util.Collections
import java.util.UUID

/** Private attempt metadata. Small indexed headers/part rows, never artifact bodies or keys. */
internal class ExportCatalogue(private val directory: File,private val fault: (String) -> Unit = {}) {
    private data class Identity(val device: Long,val inode: Long,val links: Long,val mode: Int,val uid: Int)
    private var anchor: Identity?=null
    private val file get()=File(directory,"catalogue.db")
    enum class State { Uncertain, ProviderVerified }
    data class Part(val name: String,val bytes: Long,val sha256: String,val document: Uri?=null)
    data class Record(val id: UUID,val library: UUID,val format: String,val createdMillis: Long,val tree: Uri,val folder: Uri?,
        val parts: List<Part>,val state: State,val descriptorSynced: Boolean)
    data class Attempt(val sequence: Long,val id: UUID,val library: UUID,val format: String,val createdMillis: Long,
        val tree: Uri,val folder: Uri?,val partCount: Int,val state: State,val descriptorSynced: Boolean)
    data class Page(val attempts: List<Attempt>,val next: Long?)
    private data class Header(val attempt: Attempt,val metadataBytes: Long)
    companion object { const val MAX_METADATA_BYTES=8*1024*1024 }

    /** Compatibility/full-record transition; the publisher uses incremental APIs. */
    @Synchronized fun write(record: Record,check: () -> Unit = {}) {
        validate(record);val size=encodedSize(record);require(size<=MAX_METADATA_BYTES)
        database(check) { db -> transaction(db) {
            check();val old=read(db,record.id,check)
            if(old==null) { require(record.state==State.Uncertain);insert(db,record,null,size,check) }
            else {
                require(old.library==record.library && old.format==record.format && old.createdMillis==record.createdMillis && old.tree==record.tree)
                require(old.parts.map { it.copy(document=null) }==record.parts.map { it.copy(document=null) })
                require(old.folder==null || old.folder==record.folder)
                old.parts.zip(record.parts).forEach { (a,b) -> require(a.document==null || a.document==b.document) }
                require(old.state!=State.ProviderVerified || old==record)
                val values=ContentValues().apply { put("folder",record.folder?.toString());put("state",record.state.name);put("synced",if(record.descriptorSynced) 1 else 0);put("metadata_bytes",size) }
                require(db.update("publications",values,"id=?",arrayOf(record.id.toString()))==1)
                old.parts.zip(record.parts).forEachIndexed { index,(a,b) ->
                    check();if(a.document!=b.document) require(db.update("parts",ContentValues().apply { put("document",b.document?.toString()) },"publication=? AND position=?",arrayOf(record.id.toString(),index.toString()))==1)
                }
            }
            check()
        } }
    }
    @Synchronized fun setFolder(id: UUID,library: UUID,folder: Uri,check: () -> Unit = {}) = database(check) { db -> transaction(db) {
        val h=required(db,id,library);require(h.attempt.state==State.Uncertain)
        require(h.attempt.folder==null || h.attempt.folder==folder);validateFolder(h.attempt.tree,folder)
        val size=h.metadataBytes + quoted(folder.toString())-nullableSize(h.attempt.folder)
        require(size<=MAX_METADATA_BYTES);check()
        require(db.update("publications",ContentValues().apply { put("folder",folder.toString());put("metadata_bytes",size) },"id=?",arrayOf(id.toString()))==1)
    } }
    @Synchronized fun setPart(id: UUID,library: UUID,index: Int,document: Uri,check: () -> Unit = {}) = database(check) { db -> transaction(db) {
        val h=required(db,id,library);require(h.attempt.state==State.Uncertain && index in 0 until h.attempt.partCount)
        validateDocument(h.attempt.tree,h.attempt.folder,document)
        val old=db.rawQuery("SELECT document FROM parts WHERE publication=? AND position=?",arrayOf(id.toString(),index.toString())).use { c ->
            require(c.moveToFirst());nullableUri(c,0).also { require(!c.moveToNext()) }
        }
        require(old==null || old==document)
        val size=h.metadataBytes+quoted(document.toString())-nullableSize(old);require(size<=MAX_METADATA_BYTES);check()
        require(db.update("parts",ContentValues().apply { put("document",document.toString()) },"publication=? AND position=?",arrayOf(id.toString(),index.toString()))==1)
        require(db.update("publications",ContentValues().apply { put("metadata_bytes",size) },"id=?",arrayOf(id.toString()))==1)
    } }
    @Synchronized fun verified(id: UUID,library: UUID,synced: Boolean,check: () -> Unit = {}) = database(check) { db -> transaction(db) {
        val h=required(db,id,library);require(h.attempt.folder!=null)
        if(h.attempt.state==State.ProviderVerified) require(h.attempt.descriptorSynced==synced)
        require(!db.rawQuery("SELECT 1 FROM parts WHERE publication=? AND document IS NULL LIMIT 1",arrayOf(id.toString())).use { it.moveToFirst() })
        val count=db.rawQuery("SELECT COUNT(*) FROM parts WHERE publication=?",arrayOf(id.toString())).use { require(it.moveToFirst());it.getLong(0) }
        require(count==h.attempt.partCount.toLong())
        val size=h.metadataBytes+quoted(State.ProviderVerified.name)-quoted(h.attempt.state.name)+(if(synced) 4 else 5)-(if(h.attempt.descriptorSynced) 4 else 5)
        require(size<=MAX_METADATA_BYTES);check()
        require(db.update("publications",ContentValues().apply { put("state",State.ProviderVerified.name);put("synced",if(synced) 1 else 0);put("metadata_bytes",size) },"id=?",arrayOf(id.toString()))==1)
    } }
    @Synchronized fun lookup(id: UUID,library: UUID,check: () -> Unit = {}): Record? = database(check) { db ->
        val h=header(db,id) ?: return@database null;require(h.attempt.library==library);readParts(db,h,check)
    }
    /** Header-only indexed discovery; never parses every file list to form a page. */
    @Synchronized fun history(library: UUID,before: Long?,limit: Int,check: () -> Unit = {}): Page {
        require(limit in 1..100 && (before==null || before>0))
        return database(check) { db ->
            val rows=ArrayList<Attempt>()
            val sql=if(before==null) "SELECT * FROM publications WHERE library=? ORDER BY sequence DESC LIMIT ?" else "SELECT * FROM publications WHERE library=? AND sequence<? ORDER BY sequence DESC LIMIT ?"
            val args=if(before==null) arrayOf(library.toString(),(limit+1).toString()) else arrayOf(library.toString(),before.toString(),(limit+1).toString())
            db.rawQuery(sql,args).use { c -> while(c.moveToNext()) { check();rows.add(decodeHeader(c).attempt.also { require(it.library==library) }) } }
            val more=rows.size>limit;if(more) rows.removeAt(rows.lastIndex)
            Page(Collections.unmodifiableList(rows),if(more) rows.last().sequence else null)
        }
    }
    private fun required(db: SQLiteDatabase,id: UUID,library: UUID)=requireNotNull(header(db,id)).also { require(it.attempt.library==library) }
    private fun header(db: SQLiteDatabase,id: UUID): Header? = db.rawQuery("SELECT * FROM publications WHERE id=?",arrayOf(id.toString())).use { c ->
        if(!c.moveToFirst()) null else decodeHeader(c).also { require(it.attempt.id==id && !c.moveToNext()) }
    }
    private fun decodeHeader(c: Cursor): Header {
        listOf(0,4,8,9,10).forEach { require(c.getType(it)==Cursor.FIELD_TYPE_INTEGER) }
        listOf(1,2,3,5,7).forEach { require(c.getType(it)==Cursor.FIELD_TYPE_STRING) }
        val sequence=c.getLong(0);val id=uuid(c.getString(1));val library=uuid(c.getString(2));val format=c.getString(3);val time=c.getLong(4)
        val tree=Uri.parse(c.getString(5));val folder=nullableUri(c,6);val state=State.valueOf(c.getString(7));val synced=c.getLong(8);val count=c.getLong(9);val size=c.getLong(10)
        require(sequence>0 && time>=0 && format in setOf("Native","Markdown","Pdf") && synced in 0..1 && count in 1..MAX_METADATA_BYTES.toLong() && size in 1..MAX_METADATA_BYTES.toLong())
        uri(tree);folder?.let { validateFolder(tree,it) };if(state==State.ProviderVerified) require(folder!=null)
        return Header(Attempt(sequence,id,library,format,time,tree,folder,count.toInt(),state,synced==1L),size)
    }
    private fun read(db: SQLiteDatabase,id: UUID,check: () -> Unit): Record? = header(db,id)?.let { readParts(db,it,check) }
    private fun readParts(db: SQLiteDatabase,h: Header,check: () -> Unit): Record {
        val a=h.attempt;val parts=ArrayList<Part>();var bytes=emptySize(a)+2L
        db.rawQuery("SELECT position,name,bytes,sha256,document FROM parts WHERE publication=? ORDER BY position",arrayOf(a.id.toString())).use { c ->
            while(c.moveToNext()) {
                check();require(c.getType(0)==Cursor.FIELD_TYPE_INTEGER && c.getLong(0)==parts.size.toLong())
                require(c.getType(1)==Cursor.FIELD_TYPE_STRING && c.getType(2)==Cursor.FIELD_TYPE_INTEGER && c.getType(3)==Cursor.FIELD_TYPE_STRING)
                val p=Part(c.getString(1),c.getLong(2),c.getString(3),nullableUri(c,4));validatePart(p,a.tree,a.folder)
                bytes+=partJson(p).toString().toByteArray(Charsets.UTF_8).size+(if(parts.isEmpty()) 0 else 1)
                require(bytes<=MAX_METADATA_BYTES && parts.size<a.partCount);parts.add(p)
            }
        }
        val record=Record(a.id,a.library,a.format,a.createdMillis,a.tree,a.folder,Collections.unmodifiableList(parts),a.state,a.descriptorSynced)
        require(parts.size==a.partCount);validate(record);require(encodedSize(record)==h.metadataBytes);return record
    }
    private fun insert(db: SQLiteDatabase,r: Record,sequence: Long?,size: Long,check: () -> Unit) {
        val values=ContentValues().apply {
            sequence?.let { put("sequence",it) };put("id",r.id.toString());put("library",r.library.toString());put("format",r.format);put("created_ms",r.createdMillis)
            put("tree",r.tree.toString());put("folder",r.folder?.toString());put("state",r.state.name);put("synced",if(r.descriptorSynced) 1 else 0);put("part_count",r.parts.size);put("metadata_bytes",size)
        }
        require(db.insertOrThrow("publications",null,values)>0)
        db.compileStatement("INSERT INTO parts(publication,position,name,bytes,sha256,document) VALUES(?,?,?,?,?,?)").use { statement ->
            r.parts.forEachIndexed { index,p ->
                check();statement.clearBindings();statement.bindString(1,r.id.toString());statement.bindLong(2,index.toLong());statement.bindString(3,p.name)
                statement.bindLong(4,p.bytes);statement.bindString(5,p.sha256);if(p.document==null) statement.bindNull(6) else statement.bindString(6,p.document.toString());statement.executeInsert()
            }
        }
    }
    private fun validate(r: Record) {
        require(r.format in setOf("Native","Markdown","Pdf") && r.createdMillis>=0 && r.parts.isNotEmpty());uri(r.tree);r.folder?.let { validateFolder(r.tree,it) }
        require(r.parts.map { it.name }.toSet().size==r.parts.size)
        r.parts.forEach { validatePart(it,r.tree,r.folder) }
        require(r.parts.mapNotNull { it.document }.toSet().size==r.parts.count { it.document!=null })
        if(r.state==State.ProviderVerified) require(r.folder!=null && r.parts.all { it.document!=null })
        require(encodedSize(r)<=MAX_METADATA_BYTES)
    }
    private fun validatePart(p: Part,tree: Uri,folder: Uri?) {
        require(p.name.matches(Regex("[A-Za-z0-9][A-Za-z0-9._-]{0,159}")) && p.bytes>=0 && p.sha256.matches(Regex("[0-9a-f]{64}")))
        p.document?.let { validateDocument(tree,folder,it) }
    }
    private fun uri(u: Uri) { require(u.scheme=="content" && !u.authority.isNullOrEmpty() && u.toString().length<=4096) }
    private fun validateFolder(tree: Uri,folder: Uri) { uri(folder);require(folder.authority==tree.authority && folder!=tree) }
    private fun validateDocument(tree: Uri,folder: Uri?,document: Uri) { uri(document);require(folder!=null && document!=folder && document!=tree && document.authority==tree.authority) }
    private fun uuid(s: String)=UUID.fromString(s).also { require(s.length==36 && it.toString()==s) }
    private fun nullableUri(c: Cursor,index: Int): Uri? { require(c.isNull(index) || c.getType(index)==Cursor.FIELD_TYPE_STRING);return if(c.isNull(index)) null else Uri.parse(c.getString(index)) }
    private fun quoted(s: String)=JSONObject.quote(s).toByteArray(Charsets.UTF_8).size.toLong()
    private fun nullableSize(u: Uri?)=if(u==null) 4L else quoted(u.toString())
    private fun partJson(p: Part)=JSONObject().put("name",p.name).put("bytes",p.bytes).put("sha256",p.sha256).put("document",p.document?.toString() ?: JSONObject.NULL)
    private fun encode(r: Record)=JSONObject().put("format_version",1).put("id",r.id.toString()).put("library",r.library.toString()).put("format",r.format)
        .put("created_ms",r.createdMillis).put("tree",r.tree.toString()).put("folder",r.folder?.toString() ?: JSONObject.NULL).put("state",r.state.name).put("synced",r.descriptorSynced)
        .put("parts",JSONArray(r.parts.map(::partJson)))
    private fun encodedSize(r: Record)=encode(r).toString().toByteArray(Charsets.UTF_8).size.toLong()
    private fun emptySize(a: Attempt)=encodedSize(Record(a.id,a.library,a.format,a.createdMillis,a.tree,a.folder,emptyList(),a.state,a.descriptorSynced))-2
    private fun decodeLegacy(j: JSONObject): Record {
        require(j.getInt("format_version")==1);val array=j.getJSONArray("parts")
        val r=Record(uuid(j.getString("id")),uuid(j.getString("library")),j.getString("format"),j.getLong("created_ms"),Uri.parse(j.getString("tree")),
            if(j.isNull("folder")) null else Uri.parse(j.getString("folder")),(0 until array.length()).map { i -> val p=array.getJSONObject(i)
                Part(p.getString("name"),p.getLong("bytes"),p.getString("sha256"),if(p.isNull("document")) null else Uri.parse(p.getString("document"))) },State.valueOf(j.getString("state")),j.getBoolean("synced"))
        validate(r);return r
    }
    private fun <T> transaction(db: SQLiteDatabase,action: () -> T): T {
        db.beginTransaction()
        try { val result=action();db.setTransactionSuccessful();return result } finally { db.endTransaction() }
    }
    private fun createTables(db: SQLiteDatabase) {
        db.execSQL("CREATE TABLE publications(sequence INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT NOT NULL UNIQUE,library TEXT NOT NULL,format TEXT NOT NULL,created_ms INTEGER NOT NULL,tree TEXT NOT NULL,folder TEXT,state TEXT NOT NULL,synced INTEGER NOT NULL CHECK(synced IN (0,1)),part_count INTEGER NOT NULL CHECK(part_count>0),metadata_bytes INTEGER NOT NULL CHECK(metadata_bytes>0 AND metadata_bytes<=8388608))")
        db.execSQL("CREATE INDEX publications_library_sequence ON publications(library,sequence)")
        db.execSQL("CREATE TABLE parts(publication TEXT NOT NULL REFERENCES publications(id),position INTEGER NOT NULL CHECK(position>=0),name TEXT NOT NULL,bytes INTEGER NOT NULL CHECK(bytes>=0),sha256 TEXT NOT NULL,document TEXT,PRIMARY KEY(publication,position),UNIQUE(publication,name),UNIQUE(publication,document))")
    }
    private fun migrate(db: SQLiteDatabase,check: () -> Unit) = transaction(db) {
        validateDatabase(db,1);check()
        db.execSQL("ALTER TABLE publications RENAME TO publications_v1")
        createTables(db)
        // Never put a potentially 8 MiB legacy payload into one CursorWindow row.
        db.rawQuery("SELECT sequence,id,length(CAST(payload AS BLOB)),typeof(payload) FROM publications_v1 ORDER BY sequence",null).use { rows ->
            while(rows.moveToNext()) {
                check();require(rows.getType(0)==Cursor.FIELD_TYPE_INTEGER && rows.getType(1)==Cursor.FIELD_TYPE_STRING && rows.getType(2)==Cursor.FIELD_TYPE_INTEGER && rows.getString(3)=="text")
                val sequence=rows.getLong(0);require(sequence>0);val id=uuid(rows.getString(1));val size=rows.getLong(2);require(size in 1..MAX_METADATA_BYTES.toLong())
                val bytes=ByteArray(size.toInt());var at=0
                while(at<bytes.size) {
                    check();val length=minOf(32768,bytes.size-at)
                    val chunk=db.rawQuery("SELECT substr(CAST(payload AS BLOB),?,?) FROM publications_v1 WHERE sequence=?",arrayOf((at+1).toString(),length.toString(),sequence.toString())).use { c ->
                        require(c.moveToFirst() && c.getType(0)==Cursor.FIELD_TYPE_BLOB);c.getBlob(0).also { require(it.size==length && !c.moveToNext()) }
                    }
                    chunk.copyInto(bytes,at);at+=length
                }
                val text=Charsets.UTF_8.newDecoder().onMalformedInput(CodingErrorAction.REPORT).onUnmappableCharacter(CodingErrorAction.REPORT).decode(ByteBuffer.wrap(bytes)).toString()
                val record=decodeLegacy(JSONObject(text));require(record.id==id)
                insert(db,record,sequence,encodedSize(record),check);fault("migration-record")
            }
        }
        check();fault("migration-before-commit")
        db.execSQL("DROP TABLE publications_v1")
        db.execSQL("UPDATE metadata SET value='STEM-EXPORT-CATALOGUE-2' WHERE key='purpose'")
        db.version=2
    }
    private fun <T> database(check: () -> Unit,action: (SQLiteDatabase) -> T): T {
        check();secureDirectory();checkFiles()
        val new=try { Files.createFile(file.toPath(),PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rw-------")));true }
            catch(_: java.nio.file.FileAlreadyExistsException) { false }
        checkFiles();val identity=stat(file);val flags=SQLiteDatabase.NO_LOCALIZED_COLLATORS
        if(!new) {
            SQLiteDatabase.openDatabase(file.path,null,flags or SQLiteDatabase.OPEN_READONLY).use { validateDatabase(it,it.version) }
            checkFiles();require(stat(file)==identity)
        }
        val result=SQLiteDatabase.openDatabase(file.path,null,flags or SQLiteDatabase.OPEN_READWRITE).use { db ->
            db.execSQL("PRAGMA synchronous=FULL");db.setForeignKeyConstraintsEnabled(true)
            if(new) transaction(db) {
                db.execSQL("CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL)")
                db.execSQL("INSERT INTO metadata VALUES('purpose','STEM-EXPORT-CATALOGUE-2')")
                createTables(db);db.version=2
            } else if(db.version==1) migrate(db,check)
            validateDatabase(db,2);check();action(db)
        }
        syncDirectory(directory);if(new) syncDirectory(checkNotNull(directory.parentFile));return result
    }
    private fun syncDirectory(directory: File) = java.nio.channels.FileChannel.open(directory.toPath(),java.nio.file.StandardOpenOption.READ).use { it.force(true) }
    private fun validateDatabase(db: SQLiteDatabase,version: Int) {
        require(version in 1..2 && db.version==version)
        db.rawQuery("SELECT name,type FROM sqlite_master WHERE type IN ('table','view','trigger') AND name NOT LIKE 'sqlite_%'",null).use { c ->
            val names=mutableSetOf<String>();while(c.moveToNext()) { require(c.getString(1)=="table");names.add(c.getString(0)) }
            require(names==(if(version==1) setOf("metadata","publications") else setOf("metadata","publications","parts")))
        }
        db.rawQuery("SELECT key,value FROM metadata",null).use { c ->
            require(c.moveToFirst() && c.getString(0)=="purpose" && c.getString(1)=="STEM-EXPORT-CATALOGUE-$version" && !c.moveToNext())
        }
        val expected=if(version==1) listOf("sequence","id","payload") else listOf("sequence","id","library","format","created_ms","tree","folder","state","synced","part_count","metadata_bytes")
        db.rawQuery("PRAGMA table_info(publications)",null).use { c -> val names=mutableListOf<String>();while(c.moveToNext()) names.add(c.getString(1));require(names==expected) }
        if(version==2) db.rawQuery("PRAGMA table_info(parts)",null).use { c ->
            val names=mutableListOf<String>();while(c.moveToNext()) names.add(c.getString(1));require(names==listOf("publication","position","name","bytes","sha256","document"))
        }
        if(version==2) db.rawQuery("PRAGMA index_info(publications_library_sequence)",null).use { c ->
            val names=mutableListOf<String>();while(c.moveToNext()) names.add(c.getString(2));require(names==listOf("library","sequence"))
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
