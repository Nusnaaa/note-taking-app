package org.stemnotebook.backend

import android.content.Intent
import android.content.ContextWrapper
import android.net.Uri
import android.os.CancellationSignal
import android.os.ParcelFileDescriptor
import android.provider.DocumentsContract
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import java.io.File
import java.io.IOException
import java.security.MessageDigest
import java.util.UUID
import java.util.concurrent.Callable
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/** Actual desktop JNI and host descriptors with a synthetic SAF provider. */
@RunWith(JniBackendRunner::class)
@Config(sdk=[35])
class NotebookExportsTest {
    @get:Rule val temporary=TemporaryFolder()
    private val tree=Uri.parse("content://synthetic.exports/tree/root")
    private val root=DocumentsContract.buildDocumentUriUsingTree(tree,"root")
    private val flags=Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
    private fun <T> observe(action: () -> T): T {
        val worker=Executors.newSingleThreadExecutor()
        return try { worker.submit(Callable { action() }).get(180,TimeUnit.SECONDS) } finally { worker.shutdownNow() }
    }
    private fun call(host: Long,command: String): JSONObject {
        val row=JSONObject(NativeBridge.request(host,"""{"version":1,"command":$command}"""));assertTrue(row.optString("error"),row.getBoolean("ok"));return row
    }
    private fun note(host: Long): UUID {
        val id=UUID.fromString(call(host,"""{"op":"Create","kind":"Note","parent":null,"title":"export λ"}""").getJSONObject("data").getString("id"))
        call(host,"""{"op":"Append","entity":"$id","content":{"type":"Text","value":"001 + 1 = 3"}}""")
        call(host,"""{"op":"Capture","ticket":"${UUID.randomUUID()}","entity":"$id","strokes":[{"id":"${UUID.randomUUID()}","capture_session":"${UUID.randomUUID()}","sequence":0,"pointer_id":1,"coordinate_space":"synthetic-export","tool":"Pen","points":[{"x":1.2345678901234567,"y":2.0,"time_us":1,"pressure":null,"tilt_rad":null,"orientation_rad":null,"buttons":null}]}]}""")
        return id
    }
    private inner class Documents(val directory: File): ExportDocuments() {
        val files=linkedMapOf<Uri,File>();val names=linkedMapOf<Uri,String>();val parents=linkedMapOf<Uri,Uri>()
        val written=mutableListOf<String>();var label="Selected exports";var allowed=true
        var nonempty=false;var corrupt=false;var raceAppend=false;var writeOpens=0
        var createdHook: (Uri) -> Unit={}
        val readCounts=mutableMapOf<Uri,Int>()
        override fun inspect(tree: Uri,signal: CancellationSignal): Selection {
            assertEquals("stem-exports",Thread.currentThread().name);signal.throwIfCanceled()
            if(!allowed) throw IOException("private permission diagnostic")
            return Selection(root,"root",label)
        }
        override fun create(parent: Uri,name: String,mime: String): Uri {
            assertEquals("stem-exports",Thread.currentThread().name)
            val uri=DocumentsContract.buildDocumentUriUsingTree(tree,"item-${names.size}")
            names[uri]=name;parents[uri]=parent
            if(mime!=DocumentsContract.Document.MIME_TYPE_DIR) {
                val file=File(directory,"file-${names.size}");check(file.createNewFile());files[uri]=file
                if(nonempty) file.writeText("KEEP")
            }
            createdHook(uri);return uri
        }
        override fun isChild(parent: Uri,document: Uri)=parents[document]==parent
        override fun name(document: Uri,signal: CancellationSignal)=requireNotNull(names[document])
        override fun open(document: Uri,write: Boolean,signal: CancellationSignal): ParcelFileDescriptor {
            signal.throwIfCanceled();val file=requireNotNull(files[document])
            if(write) { writeOpens++;written.add(requireNotNull(names[document]));if(raceAppend) file.writeText("KEEP") }
            else { val n=(readCounts[document] ?: 0)+1;readCounts[document]=n;if(corrupt && n==2) file.appendBytes(byteArrayOf(9)) }
            return ParcelFileDescriptor.open(file,if(write) ParcelFileDescriptor.MODE_WRITE_ONLY or ParcelFileDescriptor.MODE_APPEND else ParcelFileDescriptor.MODE_READ_ONLY)
        }
        fun bytes(name: String)=requireNotNull(files[names.entries.single { it.value==name }.key]).readBytes()
    }
    private fun failure(action: () -> Unit): NotebookExports.Failure {
        try { action();throw AssertionError("Invalid export accepted") } catch(e: NotebookExports.Failure) { assertEquals("Notebook export unavailable",e.message);return e }
    }
    private fun close(exports: NotebookExports) { exports.close();observe { exports.awaitClosed() } }
    @Test fun nativePublicationWritesManifestLastAndReconcilesAfterControllerReopen() {
        val host=NativeBridge.open(temporary.newFolder().resolve("library.db").path);val id=note(host)
        val docs=Documents(temporary.newFolder());val catalogue=temporary.newFolder().resolve("catalogue")
        var exports=NotebookExports(catalogue,{ host },docs)
        try {
            val pending=exports.reviewNative("""{"Entities":["$id"]}""",true)
            failure { pending.await() }
            val review=observe { pending.await() };val destination=observe { review.destination(tree,flags).await() }
            assertEquals("Selected exports",destination.label);assertTrue(docs.names.isEmpty())
            failure { destination.publish({ true },false,true) }
            val publication=observe { destination.publish({ true },true,true,true).await() }
            assertEquals(NotebookExports.Outcome.ProviderVerified,publication.receipt.outcome);assertTrue(publication.catalogueRecorded)
            assertNull(publication.copyRecoveryKey());assertEquals("STEM-export.json",docs.written.last())
            val bytes=docs.bytes("document.stem.json");assertTrue(bytes.toString(Charsets.UTF_8).contains("001 + 1 = 3"))
            NativeBridge.beginNativeImport(host).use { source -> assertTrue(source.prepare(bytes,null,false,30000).contains("\"strokes\":1")) }
            val manifest=JSONObject(docs.bytes("STEM-export.json").toString(Charsets.UTF_8));assertEquals(review.id.toString(),manifest.getString("operation"))
            publication.receipt.parts.forEach { part ->
                val raw=docs.bytes(part.name);assertEquals(part.bytes,raw.size.toLong());assertEquals(part.sha256,MessageDigest.getInstance("SHA-256").digest(raw).joinToString("") { "%02x".format(it.toInt() and 255) })
            }
            failure { destination.publish({ true },true,true,true) }
            close(exports);exports=NotebookExports(catalogue,{ host },docs)
            assertEquals(publication.receipt,observe { exports.reconcile(review.id).await() }.receipt);publication.close()
        } finally { close(exports);NativeBridge.close(host) }
    }
    @Test fun markdownAssetsAndPdfUseReviewedNativeOutputWithoutSourceRepair() {
        val host=NativeBridge.open(temporary.newFolder().resolve("library.db").path);val id=note(host);val docs=Documents(temporary.newFolder())
        val exports=NotebookExports(temporary.newFolder().resolve("catalogue"),{ host },docs)
        try {
            val review=observe { exports.reviewMarkdown("""{"Document":"$id"}""",true).await() }
            val destination=observe { review.destination(tree,flags).await() }
            observe { destination.publish({ true },true,true).await() }.use { publication ->
                assertEquals(NotebookExports.Outcome.ProviderVerified,publication.receipt.outcome)
                val asset=publication.receipt.parts.single { it.name.endsWith(".svg") }
                assertTrue(docs.bytes("document.md").toString(Charsets.UTF_8).contains("001 + 1 = 3"))
                assertTrue(docs.bytes("document.md").toString(Charsets.UTF_8).contains(asset.name))
                assertTrue(docs.bytes(asset.name).toString(Charsets.UTF_8).contains("1.2345678901234567"))
            }
            val font=File("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf").readBytes();val original=font.clone()
            val pdf=observe { exports.reviewPdf("""{"Document":"$id"}""",font,"""{"width_mm":210,"height_mm":297,"margin_mm":12}""").await() }
            assertArrayEquals(original,font);font.fill(0);original.fill(0);assertTrue(pdf.summary.warnings.isNotEmpty())
            val target=observe { pdf.destination(tree,flags).await() };failure { target.publish({ true },true,false) }
            observe { target.publish({ true },true,true).await() }.use { result ->
                assertEquals(NotebookExports.Outcome.ProviderVerified,result.receipt.outcome)
                assertEquals("%PDF-",docs.bytes("document.pdf").take(5).toByteArray().toString(Charsets.US_ASCII))
            }
        } finally { close(exports);NativeBridge.close(host) }
    }
    @Test fun protectedOutputRequiresFreshProofAndRevokedDestinationCreatesNothing() {
        val host=NativeBridge.open(temporary.newFolder().resolve("library.db").path);val id=note(host);val docs=Documents(temporary.newFolder())
        val exports=NotebookExports(temporary.newFolder().resolve("catalogue"),{ host },docs)
        try {
            NativeBridge.authenticate(host,"""[{"Entity":"$id"}]""") { true }
            val revision=call(host,"""{"op":"Read","id":"$id"}""").getJSONObject("data").getLong("revision")
            call(host,"""{"op":"SetLock","entity":"$id","revision":$revision,"locked":true}""")
            var review=observe { exports.reviewNative("""{"Entities":["$id"]}""",true).await() }
            var selected=observe { review.destination(tree,flags).await() }
            observe { failure { selected.publish({ false },true,true,true).await() } };assertTrue(docs.names.isEmpty())
            review=observe { exports.reviewNative("""{"Entities":["$id"]}""",true).await() };selected=observe { review.destination(tree,flags).await() }
            docs.allowed=false;observe { failure { selected.publish({ true },true,true,true).await() } };assertTrue(docs.names.isEmpty())
            docs.allowed=true
            review=observe { exports.reviewNative("""{"Entities":["$id"]}""",true).await() };selected=observe { review.destination(tree,flags).await() }
            val secret="Deliberate export provider encrypted password".toCharArray();val copy=secret.clone()
            observe { selected.publish({ check(NativeBridge.captureLibraryIdentity(host).length==36);true },true,true,false,secret).await() }.use { result ->
                assertArrayEquals(copy,secret);val key=requireNotNull(result.copyRecoveryKey());assertEquals(64,key.size)
                val artifact=docs.bytes("document.stemenc");assertEquals("STEMENC1",artifact.take(8).toByteArray().toString(Charsets.US_ASCII))
                assertFalse(docs.bytes("STEM-export.json").toString(Charsets.UTF_8).contains(String(key)));key.fill('\u0000')
                NativeBridge.beginNativeImport(host).use { source -> source.prepare(artifact,secret,false,120000) }
            };secret.fill('\u0000');copy.fill('\u0000')
        } finally { close(exports);NativeBridge.close(host) }
    }
    @Test fun providerSideEffectsAndCorruptReadbackRemainDurablyUncertain() {
        val host=NativeBridge.open(temporary.newFolder().resolve("library.db").path);val id=note(host);val docs=Documents(temporary.newFolder())
        val catalogue=temporary.newFolder().resolve("catalogue");var exports=NotebookExports(catalogue,{ host },docs)
        try {
            var review=observe { exports.reviewNative("""{"Entities":["$id"]}""",true).await() }
            var selected=observe { review.destination(tree,flags).await() }
            docs.createdHook={ throw IOException("private failure after create") }
            val uncertain=observe { selected.publish({ true },true,true,true).await() }
            assertEquals(NotebookExports.Outcome.Uncertain,uncertain.receipt.outcome);assertEquals(tree,uncertain.receipt.tree);assertNull(uncertain.receipt.folder)
            close(exports);exports=NotebookExports(catalogue,{ host },docs)
            assertEquals(uncertain.receipt,observe { exports.reconcile(review.id).await() }.receipt)
            docs.createdHook={};docs.corrupt=true
            review=observe { exports.reviewNative("""{"Entities":["$id"]}""",true).await() };selected=observe { review.destination(tree,flags).await() }
            val corrupted=observe { selected.publish({ true },true,true,true).await() }
            assertEquals(NotebookExports.Outcome.Uncertain,corrupted.receipt.outcome)
            assertFalse(docs.written.contains("STEM-export.json"));assertEquals(corrupted.receipt,observe { exports.reconcile(review.id).await() }.receipt)
            uncertain.close();corrupted.close()
        } finally { close(exports);NativeBridge.close(host) }
    }
    @Test fun nonemptyAndRacingDocumentsAreNeverExplicitlyTruncated() {
        val host=NativeBridge.open(temporary.newFolder().resolve("library.db").path);val id=note(host);val docs=Documents(temporary.newFolder())
        val exports=NotebookExports(temporary.newFolder().resolve("catalogue"),{ host },docs)
        try {
            docs.nonempty=true
            var review=observe { exports.reviewNative("""{"Entities":["$id"]}""",true).await() };var selected=observe { review.destination(tree,flags).await() }
            observe { selected.publish({ true },true,true,true).await() }.use { assertEquals(NotebookExports.Outcome.Uncertain,it.receipt.outcome) }
            assertEquals(0,docs.writeOpens);assertTrue(docs.files.values.all { it.readText()=="KEEP" })
            docs.nonempty=false;docs.raceAppend=true
            review=observe { exports.reviewNative("""{"Entities":["$id"]}""",true).await() };selected=observe { review.destination(tree,flags).await() }
            observe { selected.publish({ true },true,true,true).await() }.use { assertEquals(NotebookExports.Outcome.Uncertain,it.receipt.outcome) }
            assertTrue(docs.files.values.all { it.readText().startsWith("KEEP") })
        } finally { close(exports);NativeBridge.close(host) }
    }
    @Test fun restartDiscoversPastAttemptsWithoutProviderAccessAndAppRelockDeniesHistory() {
        val host=NativeBridge.open(temporary.newFolder().resolve("library.db").path);val id=note(host);val docs=Documents(temporary.newFolder())
        val directory=temporary.newFolder().resolve("catalogue");var exports=NotebookExports(directory,{ host },docs)
        try {
            val review=observe { exports.reviewNative("""{"Entities":["$id"]}""",true).await() };val selected=observe { review.destination(tree,flags).await() }
            val receipt=observe { selected.publish({ true },true,true,true).await() };val expected=receipt.receipt.id;receipt.close()
            close(exports);exports=NotebookExports(directory,{ host },docs)
            docs.allowed=false;docs.files.values.forEach { it.delete() }
            val page=observe { exports.history(limit=1).await() };assertEquals(expected,page.attempts.single().id);assertNull(page.next)
            assertEquals(NotebookExports.Outcome.ProviderVerified,page.attempts.single().outcome) // Past acknowledgement, not current availability.
            NativeBridge.authenticate(host,"[\"App\"]") { true };call(host,"""{"op":"SetAppLock","enabled":true}""");call(host,"""{"op":"Relock"}""")
            observe { failure { exports.history().await() } }
        } finally { close(exports);NativeBridge.close(host) }
    }
    @Test fun cataloguePreservesAttemptAndRefusesForeignOrRegressedRecords() {
        val directory=temporary.newFolder().resolve("catalogue");val catalogue=ExportCatalogue(directory)
        val id=UUID.randomUUID();val library=UUID.randomUUID();val uri=DocumentsContract.buildDocumentUriUsingTree(tree,"folder")
        val file=DocumentsContract.buildDocumentUriUsingTree(tree,"file")
        val planned=ExportCatalogue.Record(id,library,"Native",0,tree,null,listOf(ExportCatalogue.Part("document.stem.json",3,"a".repeat(64))),ExportCatalogue.State.Uncertain,false)
        catalogue.write(planned);assertEquals(planned,ExportCatalogue(directory).lookup(id,library))
        fun refused(action: () -> Unit) { try { action();fail("Invalid catalogue change accepted") } catch(_: IllegalArgumentException) { } }
        refused { catalogue.lookup(id,UUID.randomUUID()) }
        refused { catalogue.write(planned.copy(parts=listOf(planned.parts.single().copy(sha256="b".repeat(64))))) }
        val verified=planned.copy(folder=uri,parts=listOf(planned.parts.single().copy(document=file)),state=ExportCatalogue.State.ProviderVerified)
        catalogue.write(verified);assertEquals(verified,ExportCatalogue(directory).lookup(id,library))
        refused { catalogue.write(planned) };assertEquals(verified,catalogue.lookup(id,library))
    }
    @Test fun unknownCatalogueAndSymlinkNamespaceAreRefusedWithoutRepair() {
        val directory=temporary.newFolder();java.nio.file.Files.setPosixFilePermissions(directory.toPath(),java.nio.file.attribute.PosixFilePermissions.fromString("rwx------"))
        val database=File(directory,"catalogue.db")
        java.nio.file.Files.createFile(database.toPath(),java.nio.file.attribute.PosixFilePermissions.asFileAttribute(java.nio.file.attribute.PosixFilePermissions.fromString("rw-------")))
        android.database.sqlite.SQLiteDatabase.openDatabase(database.path,null,android.database.sqlite.SQLiteDatabase.OPEN_READWRITE).use { it.execSQL("CREATE TABLE original(value TEXT)");it.execSQL("INSERT INTO original VALUES('preserved')");it.version=99 }
        val bytes=database.readBytes()
        try { ExportCatalogue(directory).lookup(UUID.randomUUID(),UUID.randomUUID());fail("Unknown database accepted") } catch(_: IllegalArgumentException) { }
        assertArrayEquals(bytes,database.readBytes())
        val link=temporary.newFolder().resolve("catalogue-link");java.nio.file.Files.createSymbolicLink(link.toPath(),directory.toPath())
        try { ExportCatalogue(link).lookup(UUID.randomUUID(),UUID.randomUUID());fail("Symlink namespace accepted") } catch(_: IllegalArgumentException) { }
        assertArrayEquals(bytes,database.readBytes())
    }
    @Test fun backendCloseRetainsOwnershipThroughBlockedCreateAndPreservesOutcome() {
        val rootDir=temporary.newFolder();val context=object: ContextWrapper(RuntimeEnvironment.getApplication()) { override fun getNoBackupFilesDir(): File=rootDir }
        val docs=Documents(temporary.newFolder());val entered=CountDownLatch(1);val release=CountDownLatch(1)
        docs.createdHook={ entered.countDown();check(release.await(120,TimeUnit.SECONDS)) }
        val backend=NotebookBackend(context,{ platform,native -> PdfWorkspace(platform,native) },exportDocuments=docs)
        try {
            val result=JSONObject(backend.request("""{"version":1,"command":{"op":"Create","kind":"Note","parent":null,"title":"source"}}""").get(30,TimeUnit.SECONDS));assertTrue(result.getBoolean("ok"))
            val id=result.getJSONObject("data").getString("id");val exports=backend.exports()
            val review=observe { exports.reviewNative("""{"Entities":["$id"]}""",true).await() };val selected=observe { review.destination(tree,flags).await() }
            val pending=selected.publish({ true },true,true,true);assertTrue(entered.await(30,TimeUnit.SECONDS))
            val close=backend.closeFullyAsync();assertFalse(close.isDone)
            try { NotebookBackend(context);fail("Released ownership before provider retirement") } catch(_: IllegalStateException) { }
            release.countDown()
            observe { pending.await() }.use { assertEquals(NotebookExports.Outcome.Uncertain,it.receipt.outcome) }
            close.get(30,TimeUnit.SECONDS)
            val reopened=NotebookBackend(context);reopened.closeFullyAsync().get(30,TimeUnit.SECONDS)
        } finally { release.countDown();backend.closeFullyAsync().get(30,TimeUnit.SECONDS) }
    }
}
