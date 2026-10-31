package org.stemnotebook.backend

import android.database.sqlite.SQLiteDatabase
import android.net.Uri
import android.provider.DocumentsContract
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.io.File
import java.io.IOException
import java.nio.file.Files
import java.nio.file.attribute.PosixFilePermissions
import java.util.UUID
import java.util.concurrent.CancellationException

/** Real SQLite and filesystem; no provider access or timing/device SLO. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk=[35])
class ExportCatalogueTest {
    @get:Rule val temporary=TemporaryFolder()
    private val tree=Uri.parse("content://catalogue.test/tree/root")
    private val folder=DocumentsContract.buildDocumentUriUsingTree(tree,"folder-λ")
    private fun part(name: String="document.stem.json",document: Uri?=null)=ExportCatalogue.Part(name,3,"a".repeat(64),document)
    private fun record(library: UUID=UUID.randomUUID(),parts: List<ExportCatalogue.Part> = listOf(part())) =
        ExportCatalogue.Record(UUID.randomUUID(),library,"Native",0,tree,null,parts,ExportCatalogue.State.Uncertain,false)
    private fun encoded(r: ExportCatalogue.Record): String = JSONObject().put("format_version",1).put("id",r.id.toString()).put("library",r.library.toString())
        .put("format",r.format).put("created_ms",r.createdMillis).put("tree",r.tree.toString()).put("folder",r.folder?.toString() ?: JSONObject.NULL)
        .put("state",r.state.name).put("synced",r.descriptorSynced).put("parts",JSONArray(r.parts.map { p -> JSONObject().put("name",p.name).put("bytes",p.bytes).put("sha256",p.sha256).put("document",p.document?.toString() ?: JSONObject.NULL) })).toString()
    private fun legacy(rows: List<Triple<Long,UUID,String>>): File {
        val directory=temporary.newFolder();Files.setPosixFilePermissions(directory.toPath(),PosixFilePermissions.fromString("rwx------"))
        val file=File(directory,"catalogue.db");Files.createFile(file.toPath(),PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rw-------")))
        SQLiteDatabase.openDatabase(file.path,null,SQLiteDatabase.OPEN_READWRITE or SQLiteDatabase.NO_LOCALIZED_COLLATORS).use { db ->
            db.execSQL("CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL)")
            db.execSQL("INSERT INTO metadata VALUES('purpose','STEM-EXPORT-CATALOGUE-1')")
            db.execSQL("CREATE TABLE publications(sequence INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT NOT NULL UNIQUE,payload TEXT NOT NULL)")
            rows.forEach { (sequence,id,payload) -> db.execSQL("INSERT INTO publications VALUES(?,?,?)",arrayOf(sequence,id.toString(),payload)) }
            db.version=1
            db.rawQuery("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",null).use { c ->
                val names=mutableSetOf<String>();while(c.moveToNext()) names.add(c.getString(0));assertEquals(setOf("metadata","publications"),names)
            }
        }
        return directory
    }
    private fun version(directory: File): Int=SQLiteDatabase.openDatabase(File(directory,"catalogue.db").path,null,SQLiteDatabase.OPEN_READONLY or SQLiteDatabase.NO_LOCALIZED_COLLATORS).use { it.version }
    private fun rejected(action: () -> Unit) { try { action();fail("Invalid catalogue mutation accepted") } catch(_: IllegalArgumentException) { } }
    @Test fun realV1RowsMigrateWithSequencesUrisStatesAndNextSequencePreserved() {
        val library=UUID.randomUUID();val first=record(library)
        val second=record(library).copy(folder=folder,parts=listOf(part(document=DocumentsContract.buildDocumentUriUsingTree(tree,"file-λ"))),state=ExportCatalogue.State.ProviderVerified,descriptorSynced=true)
        val directory=legacy(listOf(Triple(5,first.id,encoded(first)),Triple(19,second.id,encoded(second))))
        val catalogue=ExportCatalogue(directory)
        assertEquals(first,catalogue.lookup(first.id,library));assertEquals(second,catalogue.lookup(second.id,library));assertEquals(2,version(directory))
        val page=catalogue.history(library,null,1);assertEquals(listOf(second.id),page.attempts.map { it.id });assertEquals(19L,page.next)
        assertEquals(listOf(first.id),catalogue.history(library,page.next,1).attempts.map { it.id })
        val next=record(library);catalogue.write(next);assertEquals(20L,catalogue.history(library,null,1).attempts.single().sequence)
    }
    @Test fun migrationFailureRollsBackAllConvertedRowsAndPurposeBeforeRetry() {
        val first=record();val second=record(first.library);val source=listOf(Triple(1L,first.id,encoded(first)),Triple(2L,second.id,encoded(second)))
        val directory=legacy(source)
        try { ExportCatalogue(directory) { if(it=="migration-record") throw IOException("injected interruption") }.lookup(first.id,first.library);fail("Interrupted migration succeeded") } catch(_: IOException) { }
        assertEquals(1,version(directory))
        SQLiteDatabase.openDatabase(File(directory,"catalogue.db").path,null,SQLiteDatabase.OPEN_READONLY or SQLiteDatabase.NO_LOCALIZED_COLLATORS).use { db ->
            db.rawQuery("SELECT sequence,id,payload FROM publications ORDER BY sequence",null).use { c -> source.forEach { row -> assertTrue(c.moveToNext());assertEquals(row.first,c.getLong(0));assertEquals(row.second.toString(),c.getString(1));assertEquals(row.third,c.getString(2)) };assertFalse(c.moveToNext()) }
            db.rawQuery("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",null).use { c -> val names=mutableSetOf<String>();while(c.moveToNext()) names.add(c.getString(0));assertEquals(setOf("metadata","publications"),names) }
        }
        assertEquals(second,ExportCatalogue(directory).lookup(second.id,second.library));assertEquals(2,version(directory))
    }
    @Test fun largeLegacyPayloadMigratesInChunksAndNormalizedRowsStayReadable() {
        val original=record(parts=(0 until 18000).map { part("asset-$it.svg") })
        val json=encoded(original);assertTrue(json.toByteArray(Charsets.UTF_8).size>2*1024*1024)
        val directory=legacy(listOf(Triple(7L,original.id,json)));var checks=0
        try { ExportCatalogue(directory).lookup(original.id,original.library) { if(++checks==10) throw CancellationException("cancelled migration chunks") };fail("Cancelled migration succeeded") } catch(_: CancellationException) { }
        assertEquals(1,version(directory));val catalogue=ExportCatalogue(directory)
        assertEquals(original,catalogue.lookup(original.id,original.library))
        var reads=0
        try { catalogue.lookup(original.id,original.library) { if(++reads==10) throw CancellationException("cancelled normalized read") };fail("Cancelled normalized lookup succeeded") } catch(_: CancellationException) { }
        assertEquals(original,catalogue.lookup(original.id,original.library));assertEquals(18000,catalogue.history(original.library,null,1).attempts.single().partCount)
        catalogue.setFolder(original.id,original.library,folder)
        val uri=DocumentsContract.buildDocumentUriUsingTree(tree,"last")
        catalogue.setPart(original.id,original.library,17999,uri)
        val restored=requireNotNull(catalogue.lookup(original.id,original.library));assertNull(restored.parts.first().document);assertEquals(uri,restored.parts.last().document)
    }
    @Test fun historyCursorIsIndexedStableAndLibraryScopedWhileNewRowsArrive() {
        val catalogue=ExportCatalogue(temporary.newFolder().resolve("catalogue"));val library=UUID.randomUUID()
        val records=(0 until 7).map { record(library) };records.forEach(catalogue::write)
        val first=catalogue.history(library,null,3);assertEquals(records.takeLast(3).reversed().map { it.id },first.attempts.map { it.id })
        catalogue.write(record());catalogue.write(record(library))
        val second=catalogue.history(library,first.next,3);val third=catalogue.history(library,second.next,3)
        assertEquals(records.reversed().map { it.id },(first.attempts+second.attempts+third.attempts).map { it.id });assertNull(third.next)
        rejected { catalogue.history(library,0,1) };rejected { catalogue.history(library,null,101) }
        val file=File(temporary.root.listFiles()!!.first { File(it,"catalogue").exists() },"catalogue/catalogue.db")
        SQLiteDatabase.openDatabase(file.path,null,SQLiteDatabase.OPEN_READONLY or SQLiteDatabase.NO_LOCALIZED_COLLATORS).use { db ->
            db.rawQuery("EXPLAIN QUERY PLAN SELECT * FROM publications WHERE library=? AND sequence<? ORDER BY sequence DESC LIMIT ?",arrayOf(library.toString(),"999","4")).use { c ->
                val plans=mutableListOf<String>();while(c.moveToNext()) plans.add(c.getString(3));assertTrue(plans.toString(),plans.any { it.contains("publications_library_sequence") });assertFalse(plans.toString(),plans.any { it.contains("TEMP B-TREE") })
            }
        }
    }
    @Test fun duplicateIncrementalBindingRollsBackAndCannotUpgradeIncompleteAttempt() {
        val catalogue=ExportCatalogue(temporary.newFolder().resolve("catalogue"));val original=record(parts=listOf(part("first"),part("second")))
        catalogue.write(original);catalogue.setFolder(original.id,original.library,folder)
        val uri=DocumentsContract.buildDocumentUriUsingTree(tree,"one");catalogue.setPart(original.id,original.library,0,uri)
        try { catalogue.setPart(original.id,original.library,1,uri);fail("Duplicate document accepted") } catch(_: android.database.sqlite.SQLiteConstraintException) { }
        val after=requireNotNull(catalogue.lookup(original.id,original.library));assertEquals(uri,after.parts[0].document);assertNull(after.parts[1].document)
        rejected { catalogue.verified(original.id,original.library,true) };assertEquals(ExportCatalogue.State.Uncertain,catalogue.history(original.library,null,1).attempts.single().state)
        rejected { catalogue.setFolder(original.id,UUID.randomUUID(),folder) }
        assertEquals(after,catalogue.lookup(original.id,original.library))
    }
    @Test fun incrementalMetadataBudgetRefusalDoesNotChangeTheStoredPlan() {
        val library=UUID.randomUUID();val id=UUID.randomUUID()
        val base=record(library).copy(id=id,parts=emptyList())
        val sample=part("x".repeat(160));val partBytes=JSONObject().put("name",sample.name).put("bytes",sample.bytes).put("sha256",sample.sha256).put("document",JSONObject.NULL).toString().toByteArray(Charsets.UTF_8).size
        val count=(ExportCatalogue.MAX_METADATA_BYTES-encoded(base).toByteArray(Charsets.UTF_8).size)/(partBytes+1)
        val original=base.copy(parts=(0 until count).map { part(it.toString().padEnd(160,'x')) })
        assertTrue(encoded(original).toByteArray(Charsets.UTF_8).size<=ExportCatalogue.MAX_METADATA_BYTES)
        val catalogue=ExportCatalogue(temporary.newFolder().resolve("catalogue"));catalogue.write(original)
        val longFolder=Uri.parse("content://catalogue.test/tree/root/document/"+"x".repeat(3500))
        rejected { catalogue.setFolder(id,library,longFolder) }
        assertEquals(original,catalogue.lookup(id,library))
    }
    @Test fun malformedLegacyRecordNeverLeavesAPartiallyNormalizedDatabase() {
        val first=record();val second=record(first.library);val bad=JSONObject(encoded(second));bad.getJSONArray("parts").getJSONObject(0).put("sha256","invalid")
        val directory=legacy(listOf(Triple(1L,first.id,encoded(first)),Triple(2L,second.id,bad.toString())))
        rejected { ExportCatalogue(directory).lookup(first.id,first.library) };assertEquals(1,version(directory))
        SQLiteDatabase.openDatabase(File(directory,"catalogue.db").path,null,SQLiteDatabase.OPEN_READONLY or SQLiteDatabase.NO_LOCALIZED_COLLATORS).use { db ->
            db.rawQuery("SELECT payload FROM publications WHERE id=?",arrayOf(second.id.toString())).use { c -> assertTrue(c.moveToFirst());assertEquals(bad.toString(),c.getString(0)) }
        }
    }
}
