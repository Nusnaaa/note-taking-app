package org.stemnotebook.backend

import android.content.ContextWrapper
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import java.io.File
import java.util.UUID
import java.util.concurrent.Callable
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference

/** Real desktop JNI in API-35 simulation; app-owned synthetic providers only. */
@RunWith(JniBackendRunner::class)
@Config(sdk = [35])
class NotebookEnginesTest {
    @get:Rule val temporary = TemporaryFolder()
    private fun <T> observer(action: () -> T): T {
        val executor = Executors.newSingleThreadExecutor()
        return try { executor.submit(Callable { action() }).get(30, TimeUnit.SECONDS) }
        finally { executor.shutdownNow() }
    }
    private fun descriptor(version: String = "1") = """{"id":"synthetic-kotlin","version":"$version","adapter_version":"1","family":"Recogniser","input":"OrderedStrokesV1","tasks":["Prose"],"internals":"StrokeOnlyVerified","offline":true,"content_egress":false,"personal_use_evidence":"app-owned Kotlin JNI fixture; no real model","redistributable_resources":false,"resource_sha256":null,"runtime_contract":"stem-processing-v1"}"""
    private fun native(host: Long, command: JSONObject): JSONObject {
        val result = JSONObject(NativeBridge.request(host, JSONObject().put("version",1).put("command",command).toString()))
        assertTrue(result.optString("error"),result.getBoolean("ok")); return result
    }
    private fun failure(action: () -> Unit): NotebookEngines.Failure {
        try { action(); throw AssertionError("accepted invalid engine operation") }
        catch (error: NotebookEngines.Failure) { assertFalse(error.message.orEmpty().contains("private"));return error }
    }
    private open class Model : NativeBridge.Recogniser {
        val retired = AtomicInteger()
        val active = AtomicReference<NativeBridge.EngineControl>()
        override fun probe(control: NativeBridge.EngineControl) { assertEquals("stem-engines",Thread.currentThread().name);control.check();active.set(control) }
        override fun recognise(orderedInk: String,task: String,control: NativeBridge.EngineControl): String {
            assertEquals("stem-engines",Thread.currentThread().name);control.check();active.set(control)
            assertEquals("Prose",task);assertTrue(orderedInk.contains("\"strokes\":["))
            return """{"candidates":[{"type":"Text","value":"1 + 1 = 3"}],"raw_output":"1 + 1 = 3","confidence":null,"diagnostics":[]}"""
        }
        override fun retire() { assertEquals(1,retired.incrementAndGet()) }
    }
    private fun factory(model: Model) = NativeBridge.RecogniserFactory { descriptor,resource,control ->
        assertEquals("stem-engines",Thread.currentThread().name);assertTrue(descriptor.contains("OrderedStrokesV1"));assertNull(resource);control.check();model
    }
    private fun inkCommand(note: UUID) = JSONObject("""{"op":"Capture","ticket":"${UUID.randomUUID()}","entity":"$note","strokes":[{"id":"${UUID.randomUUID()}","capture_session":"${UUID.randomUUID()}","sequence":0,"pointer_id":1,"coordinate_space":"synthetic-local-units","tool":"Pen","end":"Completed","points":[{"x":0.1,"y":0.2,"time_us":10,"pressure":null,"tilt_rad":null,"orientation_rad":null,"buttons":null},{"x":1.1,"y":1.2,"time_us":20,"pressure":0.0,"tilt_rad":null,"orientation_rad":null,"buttons":0}]}]}""")
    @Test fun catalogueTracksPagingDefaultAndExplicitReload() {
        val path = temporary.newFolder().resolve("library.db").absolutePath
        var host = NativeBridge.open(path)
        var engines = NotebookEngines { host }
        try {
            failure { engines.catalogue(limit = 0) }
            val staged = observer { engines.stageResourceFree(descriptor(), true).await() }
            val other = observer { engines.stageResourceFree(descriptor("2"), true).await() }
            val first = observer { engines.catalogue(limit = 1).await() }
            assertEquals(1, first.versions.size); assertNotNull(first.next)
            val second = observer { engines.catalogue(first.next, 1).await() }
            assertNull(second.next)
            assertEquals(setOf(staged.version,other.version), (first.versions + second.versions).map { it.id }.toSet())
            assertFalse(first.versions[0].runtimeReady)
            assertFalse(first.versions[0].descriptorJson.contains("local_resource"))
            observer { failure { engines.selectDefault(NotebookEngines.Task.Prose,"synthetic-kotlin").await() } }
            observer { engines.activate(staged.version,0,30000,factory(Model())).await() }
            observer { engines.selectDefault(NotebookEngines.Task.Prose,"synthetic-kotlin").await() }
            val ready = observer { engines.catalogue().await() }
            assertEquals(1,ready.versions.count { it.runtimeReady })
            assertTrue(ready.defaults.single().runtimeReady)
            val note = UUID.fromString(native(host,JSONObject().put("op","Create").put("kind","Note").put("parent",JSONObject.NULL).put("title","default fixture")).getJSONObject("data").getString("id"))
            val block = UUID.fromString(native(host,inkCommand(note)).getJSONObject("data").getString("id"))
            observer { engines.recognise(block,null,NotebookEngines.Task.Prose,null,30000).await() }
            engines.close(); NativeBridge.close(host)
            host = NativeBridge.open(path); engines = NotebookEngines { host }
            val unloaded = observer { engines.catalogue().await() }
            assertEquals(1,unloaded.versions.count { it.active })
            assertFalse(unloaded.versions.any { it.runtimeReady })
            assertFalse(unloaded.defaults.single().runtimeReady)
            observer { engines.activate(staged.version,0,30000,factory(Model())).await() }
            assertTrue(observer { engines.catalogue().await() }.defaults.single().runtimeReady)
            observer { engines.recognise(block,null,NotebookEngines.Task.Prose,null,30000).await() }
        } finally { engines.close(); NativeBridge.close(host) }
    }
    @Test fun workerWorkflowPreservesSourceAndKnownReceiptsAfterLateCancellation() {
        val host = NativeBridge.open(temporary.newFolder().resolve("library.db").absolutePath)
        val engines = NotebookEngines {host}
        val model = Model()
        try {
            val note = UUID.fromString(native(host,JSONObject().put("op","Create").put("kind","Note").put("parent",JSONObject.NULL).put("title","synthetic ink")).getJSONObject("data").getString("id"))
            val block = UUID.fromString(native(host,inkCommand(note)).getJSONObject("data").getString("id"))
            val source = JSONObject().put("op","SourceInk").put("block",block.toString())
            val original = native(host,source).get("data").toString()
            val staged = engines.stageResourceFree(descriptor(),true)
            assertEquals(NotebookEngines.FailureReason.Unavailable,failure {staged.await()}.reason)
            val version = observer {staged.await()}.version
            assertFalse(staged.cancel());assertEquals(version,observer {staged.await()}.version)
            val activation = engines.activate(version,0,30000,factory(model))
            assertEquals(version,observer {activation.await()}.version)
            assertFalse(activation.cancel());assertEquals(version,observer {activation.await()}.version)
            assertFalse(model.active.get().active())
            val proposal = observer {engines.recognise(block,null,NotebookEngines.Task.Prose,"synthetic-kotlin",30000).await()}
            assertEquals(block,proposal.block)
            val stored = native(host,JSONObject().put("op","Proposal").put("proposal",proposal.id.toString())).getJSONObject("data")
            assertEquals("Pending",stored.getString("state"));assertEquals("1 + 1 = 3",stored.getJSONObject("output").getString("raw_output"))
            assertEquals(original,native(host,source).get("data").toString())
        } finally {engines.close();NativeBridge.close(host);observer {engines.awaitClosed()}}
        assertEquals(1,model.retired.get())
    }
    @Test fun queueBoundsAndQueuedCancellationDoNotConstructAnotherSdk() {
        val host = NativeBridge.open(temporary.newFolder().resolve("library.db").absolutePath)
        val engines = NotebookEngines {host}
        val entered = CountDownLatch(1);val release = CountDownLatch(1)
        val secondCalls = AtomicInteger()
        val model = object : Model() {
            override fun probe(control: NativeBridge.EngineControl) {super.probe(control);entered.countDown();assertTrue(release.await(10,TimeUnit.SECONDS));control.check()}
        }
        try {
            val version = observer {engines.stageResourceFree(descriptor(),true).await()}.version
            val first = engines.activate(version,0,30000,factory(model))
            assertTrue(entered.await(10,TimeUnit.SECONDS))
            val queued = engines.activate(version,0,30000) {_,_,_ -> secondCalls.incrementAndGet();Model()}
            assertTrue(queued.cancel())
            assertEquals(NotebookEngines.FailureReason.Busy,failure {engines.activate(version,0,30000,factory(Model()))}.reason)
            assertTrue(native(host,JSONObject().put("op","Status")).getBoolean("ok"))
            release.countDown();assertEquals(version,observer {first.await()}.version)
            val cancelled = observer {failure {queued.await()}}
            assertEquals(NotebookEngines.FailureReason.Cancelled,cancelled.reason);assertFalse(cancelled.metadataMayHaveChanged)
            assertEquals(0,secondCalls.get())
        } finally {release.countDown();engines.close();NativeBridge.close(host);observer {engines.awaitClosed()}}
    }
    @Test fun failedProviderIsRedactedRetiredAndDoesNotStrandTheOptionalWorker() {
        val host = NativeBridge.open(temporary.newFolder().resolve("library.db").absolutePath)
        val engines = NotebookEngines {host};val retired = AtomicInteger();val valid = Model()
        try {
            val version = observer {engines.stageResourceFree(descriptor(),true).await()}.version
            val attempted = engines.activate(version,0,30000) {_,_,_ -> object : Model() {
                override fun probe(control: NativeBridge.EngineControl) {throw IllegalArgumentException("private provider diagnostic")}
                override fun retire() {retired.incrementAndGet()}
            }}
            val unavailable = observer {failure {attempted.await()}}
            assertEquals(NotebookEngines.FailureReason.Unavailable,unavailable.reason);assertTrue(unavailable.metadataMayHaveChanged)
            assertEquals(1,retired.get())
            assertEquals(version,observer {engines.activate(version,0,30000,factory(valid)).await()}.version)
            assertEquals(NotebookEngines.FailureReason.InvalidInput,failure {engines.activate(version,-1,30000,factory(valid))}.reason)
        } finally {engines.close();NativeBridge.close(host);observer {engines.awaitClosed()}}
    }
    @Test fun backendCloseKeepsOwnershipThroughBlockedInferenceAndSdkRetirement(): Unit = observer {
        val root = temporary.newFolder()
        val context = object : ContextWrapper(RuntimeEnvironment.getApplication()) {override fun getNoBackupFilesDir(): File = root}
        val backend = NotebookBackend(context)
        val entered = CountDownLatch(1);val release = CountDownLatch(1);val retiring = CountDownLatch(1);val releaseRetire = CountDownLatch(1)
        fun call(command: JSONObject): JSONObject {
            val result = JSONObject(backend.request(JSONObject().put("version",1).put("command",command).toString()).get(10,TimeUnit.SECONDS))
            assertTrue(result.optString("error"),result.getBoolean("ok"));return result
        }
        val model = object : Model() {
            override fun recognise(ink: String,task: String,control: NativeBridge.EngineControl): String {
                val output = super.recognise(ink,task,control);entered.countDown();assertTrue(release.await(10,TimeUnit.SECONDS));control.check();return output
            }
            override fun retire() {super.retire();retiring.countDown();assertTrue(releaseRetire.await(10,TimeUnit.SECONDS))}
        }
        try {
            val note = UUID.fromString(call(JSONObject().put("op","Create").put("kind","Note").put("parent",JSONObject.NULL).put("title","controller ownership")).getJSONObject("data").getString("id"))
            val block = UUID.fromString(call(inkCommand(note)).getJSONObject("data").getString("id"))
            val engines = backend.engines()
            val version = engines.stageResourceFree(descriptor(),true).await().version
            engines.activate(version,0,30000,factory(model)).await()
            val pending = engines.recognise(block,null,NotebookEngines.Task.Prose,"synthetic-kotlin",30000)
            assertTrue(entered.await(10,TimeUnit.SECONDS));assertTrue(call(JSONObject().put("op","Status")).getBoolean("ok"))
            val closing = backend.closeFullyAsync()
            assertFalse(model.active.get().active());assertFalse(closing.isDone)
            assertThrows(IllegalStateException::class.java) {LibraryOwnership.acquire(root)}
            release.countDown();assertTrue(retiring.await(10,TimeUnit.SECONDS))
            assertFalse(closing.isDone);assertThrows(IllegalStateException::class.java) {LibraryOwnership.acquire(root)}
            releaseRetire.countDown();closing.get(20,TimeUnit.SECONDS)
            assertEquals(NotebookEngines.FailureReason.Cancelled,failure {pending.await()}.reason)
            assertEquals(1,model.retired.get());LibraryOwnership.acquire(root).close()
            assertEquals(NotebookEngines.FailureReason.Closed,failure {engines.stageResourceFree(descriptor(),true)}.reason)
        } finally {release.countDown();releaseRetire.countDown();backend.closeFullyAsync().get(20,TimeUnit.SECONDS)}
    }
    @Test fun engineOwnersUseTheJniParentLoader() {
        assertSame(NativeBridge::class.java.classLoader,NativeBridge.EngineRecovery::class.java.classLoader)
        assertSame(NativeBridge::class.java.classLoader,NativeBridge.EngineRemoval::class.java.classLoader)
        assertSame(NativeBridge::class.java.classLoader,NativeBridge.EngineDiscard::class.java.classLoader)
        assertSame(NativeBridge::class.java.classLoader,NativeBridge.EngineReclamation::class.java.classLoader)
    }
    @Test fun removalPreservesAcceptedContentAndKnownReceiptAcrossReopen() {
        val path=temporary.newFolder().resolve("library.db").absolutePath
        var host=NativeBridge.open(path);var engines=NotebookEngines {host}
        val model=Model()
        try {
            val version=observer {engines.stageResourceFree(descriptor(),true).await()}.version
            observer {engines.activate(version,0,30000,factory(model)).await()}
            observer {engines.selectDefault(NotebookEngines.Task.Prose,"synthetic-kotlin").await()}
            val note=UUID.fromString(native(host,JSONObject().put("op","Create").put("kind","Note").put("parent",JSONObject.NULL).put("title","accepted removal fixture")).getJSONObject("data").getString("id"))
            val block=UUID.fromString(native(host,inkCommand(note)).getJSONObject("data").getString("id"))
            val source=JSONObject().put("op","SourceInk").put("block",block.toString());val raw=native(host,source).get("data").toString()
            val proposal=observer {engines.recognise(block,null,NotebookEngines.Task.Prose,null,30000).await()}
            native(host,JSONObject().put("op","AcceptProposal").put("proposal",proposal.id.toString()).put("candidate",0))
            val inactive=observer {engines.stageResourceFree(descriptor("2"),true).await()}.version
            observer {engines.remove(inactive,30000).await()};assertEquals(0,model.retired.get())
            assertTrue(observer {engines.catalogue().await()}.versions.any {it.runtimeReady})
            val pending=engines.remove(version,30000);assertEquals(version,observer {pending.await()}.version)
            assertFalse(pending.cancel());assertEquals(version,observer {pending.await()}.version)
            assertEquals(version,observer {engines.remove(version,30000).await()}.version)
            val catalogue=observer {engines.catalogue().await()}
            assertFalse(catalogue.versions.any {it.runtimeReady});assertTrue(catalogue.defaults.isEmpty())
            assertTrue(catalogue.versions.all {it.installation==NotebookEngines.Installation.Removed})
            assertEquals(raw,native(host,source).get("data").toString())
            assertTrue(native(host,JSONObject().put("op","Block").put("block",block.toString())).get("data").toString().contains("1 + 1 = 3"))
            observer {failure {engines.recognise(block,null,NotebookEngines.Task.Prose,null,30000).await()}}
            engines.close();NativeBridge.close(host);observer {engines.awaitClosed()};assertEquals(1,model.retired.get())
            host=NativeBridge.open(path);engines=NotebookEngines {host}
            assertTrue(observer {engines.catalogue().await()}.versions.all {it.installation==NotebookEngines.Installation.Removed && !it.runtimeReady})
            assertEquals(raw,native(host,source).get("data").toString())
        } finally {engines.close();NativeBridge.close(host);observer {engines.awaitClosed()}}
    }
    @Test fun removalAcknowledgesMetadataWhileSdkRetiresAndCloseKeepsOwnership() {
        val path=temporary.newFolder().resolve("library.db").absolutePath;val host=NativeBridge.open(path)
        val engines=NotebookEngines {host};val entered=CountDownLatch(1);val release=CountDownLatch(1)
        val closer=Executors.newSingleThreadExecutor()
        val model=object:Model() {override fun retire(){super.retire();entered.countDown();assertTrue(release.await(10,TimeUnit.SECONDS))}}
        try {
            val version=observer {engines.stageResourceFree(descriptor(),true).await()}.version
            observer {engines.activate(version,0,30000,factory(model)).await()}
            val removed=observer {engines.remove(version,30000).await()};assertEquals(version,removed.version)
            assertTrue(entered.await(10,TimeUnit.SECONDS))
            native(host,JSONObject().put("op","Create").put("kind","Note").put("parent",JSONObject.NULL).put("title","authoring during cleanup"))
            engines.close();val closing=closer.submit {NativeBridge.close(host)}
            val until=System.nanoTime()+TimeUnit.SECONDS.toNanos(10)
            while(true){try {NativeBridge.recognitionStatus(host)}catch(closed:IllegalStateException){break};assertTrue(System.nanoTime()<until);Thread.yield()}
            assertFalse(closing.isDone);assertThrows(IllegalStateException::class.java) {NativeBridge.open(path)}
            release.countDown();closing.get(10,TimeUnit.SECONDS);assertEquals(1,model.retired.get())
            val reopened=NativeBridge.open(path);NativeBridge.close(reopened)
        } finally {
            release.countDown();engines.close();try {NativeBridge.close(host)}catch(closed:IllegalStateException){}
            closer.shutdownNow();assertTrue(closer.awaitTermination(10,TimeUnit.SECONDS));observer {engines.awaitClosed()}
        }
    }
    @Test fun recoveryIsReadOnlyRedactedLibraryBoundAndSurvivesReopen() {
        val path=temporary.newFolder().resolve("library.db").absolutePath
        val models=temporary.newFolder("recovery-depot")
        var host=NativeBridge.open(path);var engines=NotebookEngines(models) {host}
        val bytes=ByteArray(70001) {(it*71+137).toByte()}
        val sha=java.security.MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") {"%02x".format(it.toInt() and 255)}
        val manifest=JSONObject(descriptor()).put("resource_sha256",sha).toString()
        val receipt=AtomicReference<NotebookEngines.Transfer>();val retired=AtomicInteger()
        try {
            failure {engines.recover(limit=0)}
            val pending=engines.importResource(manifest,bytes.size.toLong(),true,null,30000,{receipt.set(it)}) { _,_ ->
                object:NativeBridge.EngineInput {
                    var read=false
                    override fun read(buffer:ByteArray,control:NativeBridge.EngineControl):Int {
                        control.check();if(read)throw IllegalArgumentException("private interrupted source");read=true
                        bytes.copyInto(buffer,0,0,buffer.size);return buffer.size
                    }
                    override fun retire(){assertEquals(1,retired.incrementAndGet())}
                }
            }
            observer {failure {pending.await()}}
            assertEquals(1,retired.get())
            val id=receipt.get().version;val part=File(models,"$id.part");part.appendBytes(byteArrayOf(1,2,3))
            val before=part.readBytes()
            val first=observer {engines.recover().await()}.entries.single()
            assertEquals(id,first.id);assertEquals(65536L,first.committedBytes)
            assertEquals(NotebookEngines.RecoveryState.Interrupted,first.state);assertNotNull(first.consentMillis)
            assertFalse(first.descriptorJson!!.contains(models.absolutePath));assertArrayEquals(before,part.readBytes())
            val foreign=NativeBridge.open(temporary.newFolder().resolve("foreign.db").absolutePath)
            val other=NotebookEngines(models) {foreign}
            try {assertTrue(observer {other.recover().await()}.entries.isEmpty())}
            finally {other.close();NativeBridge.close(foreign);observer {other.awaitClosed()}}
            val legacy=UUID.randomUUID();val corrupt=UUID.randomUUID()
            val ledger=JSONObject(File(models,"$id.install.json").readText()).put("id",legacy.toString());ledger.remove("binding")
            File(models,"$legacy.install.json").writeText(ledger.toString());File(models,"$corrupt.install.json").writeText("private invalid metadata")
            var after:UUID?=null;val entries=mutableListOf<NotebookEngines.RecoveryEntry>()
            do {val page=observer {engines.recover(after,1).await()};entries.addAll(page.entries);after=page.next} while(after!=null)
            assertEquals(setOf(id,legacy,corrupt),entries.map {it.id}.toSet())
            assertEquals(NotebookEngines.RecoveryState.ReviewRequired,entries.single {it.id==legacy}.state)
            assertEquals(NotebookEngines.RecoveryState.Invalid,entries.single {it.id==corrupt}.state)
            entries.filter {it.id!=id}.forEach {assertNull(it.descriptorJson);assertNull(it.committedBytes);assertNull(it.consentMillis)}
            engines.close();NativeBridge.close(host);observer {engines.awaitClosed()}
            host=NativeBridge.open(path);engines=NotebookEngines(models) {host}
            assertEquals(3,observer {engines.recover().await()}.entries.size)
            assertArrayEquals(before,part.readBytes());assertTrue(observer {engines.catalogue().await()}.versions.isEmpty())
        } finally {engines.close();NativeBridge.close(host);observer {engines.awaitClosed()}}
    }
    @Test fun resourceImportPublishesStableOffsetBeforeOpeningAndClearsVerifiedFactoryBytes() {
        val host=NativeBridge.open(temporary.newFolder().resolve("library.db").absolutePath)
        val models=temporary.newFolder("model-depot")
        val engines=NotebookEngines(models) {host}
        val bytes=ByteArray(70001) {(it*71+137).toByte()}
        val sha=java.security.MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") {"%02x".format(it.toInt() and 255)}
        val manifest=JSONObject(descriptor()).put("resource_sha256",sha).toString()
        val receipt=AtomicReference<NotebookEngines.Transfer>();val opened=AtomicInteger();val retired=AtomicInteger();val arrays=mutableListOf<ByteArray>();val factoryArray=AtomicReference<ByteArray>()
        try {
            val pending=engines.importResource(manifest,bytes.size.toLong(),true,null,30000,{ value -> assertEquals(0,opened.get());receipt.set(value) }) { transfer, control ->
                assertEquals("stem-engines",Thread.currentThread().name);control.check();assertEquals(receipt.get(),transfer);assertEquals(0L,transfer.committedBytes);opened.incrementAndGet()
                object:NativeBridge.EngineInput {
                    var offset=0
                    override fun read(buffer:ByteArray,current:NativeBridge.EngineControl):Int {current.check();arrays.add(buffer);if(offset==bytes.size)return -1;val n=minOf(buffer.size,bytes.size-offset);bytes.copyInto(buffer,0,offset,offset+n);offset+=n;return n}
                    override fun retire(){assertEquals(1,retired.incrementAndGet())}
                }
            }
            val staged=observer {pending.await()};assertEquals(receipt.get().version,staged.version);assertEquals(1,opened.get());assertEquals(1,retired.get());arrays.forEach {assertTrue(it.all {value->value==0.toByte()})}
            val model=Model()
            observer {engines.activate(staged.version,bytes.size.toLong(),30000) { _,resource,control -> control.check();assertArrayEquals(bytes,resource);factoryArray.set(resource);model }.await()}
            assertTrue(factoryArray.get().all {it==0.toByte()});assertArrayEquals(bytes,File(models,"${staged.version}.resource").readBytes())
        } finally {engines.close();NativeBridge.close(host);observer {engines.awaitClosed()}}
    }
    @Test fun resourceInputOpeningRemainsOwnedAcrossControllerAndBackendClose(): Unit = observer {
        val root=temporary.newFolder();val context=object:ContextWrapper(RuntimeEnvironment.getApplication()){override fun getNoBackupFilesDir():File=root}
        val backend=NotebookBackend(context);val entered=CountDownLatch(1);val release=CountDownLatch(1);val retiring=CountDownLatch(1);val releaseRetire=CountDownLatch(1);val current=AtomicReference<NativeBridge.EngineControl>();val calls=AtomicInteger()
        val bytes="app-owned resource opening fixture".toByteArray();val sha=java.security.MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") {"%02x".format(it.toInt() and 255)};val manifest=JSONObject(descriptor()).put("resource_sha256",sha).toString()
        try {
            val pending=backend.engines().importResource(manifest,bytes.size.toLong(),true,null,30000) { _,control ->
                current.set(control);entered.countDown();assertTrue(release.await(10,TimeUnit.SECONDS))
                object:NativeBridge.EngineInput {
                    override fun read(b:ByteArray,c:NativeBridge.EngineControl):Int {throw AssertionError("cancelled input read")}
                    override fun retire(){assertEquals(1,calls.incrementAndGet());retiring.countDown();assertTrue(releaseRetire.await(10,TimeUnit.SECONDS))}
                }
            }
            assertTrue(entered.await(10,TimeUnit.SECONDS));val closing=backend.closeFullyAsync();assertFalse(current.get().active());assertFalse(closing.isDone);assertThrows(IllegalStateException::class.java){LibraryOwnership.acquire(root)}
            release.countDown();assertTrue(retiring.await(10,TimeUnit.SECONDS));assertFalse(closing.isDone);assertThrows(IllegalStateException::class.java){LibraryOwnership.acquire(root)}
            releaseRetire.countDown();closing.get(20,TimeUnit.SECONDS);assertEquals(NotebookEngines.FailureReason.Cancelled,failure {pending.await()}.reason);assertEquals(1,calls.get());LibraryOwnership.acquire(root).close()
        } finally {release.countDown();releaseRetire.countDown();backend.closeFullyAsync().get(20,TimeUnit.SECONDS)}
    }

    private fun interruptedDiscardFixture(engines: NotebookEngines): UUID {
        val bytes = "model".toByteArray()
        val sha = java.security.MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it.toInt() and 255) }
        val manifest = JSONObject(descriptor()).put("resource_sha256",sha).toString()
        val transfer = AtomicReference<NotebookEngines.Transfer>()
        val pending = engines.importResource(manifest,5,true,null,30000,{ transfer.set(it) }) { _,_ ->
            object: NativeBridge.EngineInput {
                var reads = 0
                override fun read(buffer: ByteArray,control: NativeBridge.EngineControl): Int {
                    control.check(); if (reads++ != 0) throw IllegalArgumentException("private fixture interruption")
                    buffer[0] = 'm'.code.toByte(); buffer[1] = 'o'.code.toByte(); return 2
                }
                override fun retire() {}
            }
        }
        observer { failure { pending.await() } }
        return transfer.get().version
    }
    @Test fun explicitDiscardReturnsDurableTypedHistoryAndRetainsStableActionIdentity() {
        val host = NativeBridge.open(temporary.newFolder().resolve("library.db").absolutePath)
        val models = temporary.newFolder("discard-models"); val engines = NotebookEngines(models) { host }
        try {
            val version = interruptedDiscardFixture(engines)
            assertEquals(2L,File(models,"$version.part").length())
            assertEquals(NotebookEngines.FailureReason.InvalidInput, failure { engines.discardImport(version,false) }.reason)
            val pending = engines.discardImport(version,true)
            val receipt = observer { pending.await() }
            assertEquals(pending.resourceActionId,receipt.id); assertEquals(version,receipt.version)
            assertEquals(NotebookEngines.ResourceActionState.Completed,receipt.state)
            assertEquals(2L,receipt.committedBytes);assertEquals(5L,receipt.totalBytes)
            assertFalse(File(models,"$version.part").exists());assertFalse(File(models,"$version.install.json").exists())
            assertTrue(File(models,"$version.owner.lock").isFile)
            assertEquals(receipt,observer { engines.resourceAction(receipt.id).await() })
            val page = observer { engines.resourceActions(limit=1).await() }
            assertEquals(listOf(receipt),page.actions); assertNull(page.next)
            assertThrows(UnsupportedOperationException::class.java) { (page.actions as MutableList).clear() }
            assertTrue(observer { engines.catalogue().await() }.versions.isEmpty())
        } finally { engines.close();NativeBridge.close(host);observer { engines.awaitClosed() } }
    }
    @Test fun failedDiscardPreservesActionIdentityWithoutPromisingRollbackOrInventingPendingHistory() {
        val host = NativeBridge.open(temporary.newFolder().resolve("library.db").absolutePath)
        val models = temporary.newFolder("discard-invalid"); val engines = NotebookEngines(models) { host }
        try {
            val version = interruptedDiscardFixture(engines)
            val part = File(models,"$version.part"); val outside = temporary.newFile("independent").apply { writeText("outside source") }
            assertTrue(part.delete());java.nio.file.Files.createSymbolicLink(part.toPath(),outside.toPath())
            val pending = engines.discardImport(version,true)
            val error = observer { failure { pending.await() } }
            assertTrue(error.metadataMayHaveChanged);assertNotNull(error.resourceActionId)
            assertEquals(pending.resourceActionId,error.resourceActionId)
            assertEquals("outside source",outside.readText());assertTrue(java.nio.file.Files.isSymbolicLink(part.toPath()))
            assertTrue(observer { engines.resourceActions().await() }.actions.isEmpty())
            val unknown = observer { failure { engines.resourceAction(error.resourceActionId!!).await() } }
            assertFalse(unknown.metadataMayHaveChanged)
            assertTrue(part.delete());part.writeText("mo")
            val fresh = observer { engines.discardImport(version,true).await() }
            assertNotEquals(error.resourceActionId,fresh.id)
            assertEquals(NotebookEngines.ResourceActionState.Completed,fresh.state)
            assertEquals("outside source",outside.readText())
        } finally { engines.close();NativeBridge.close(host);observer { engines.awaitClosed() } }
    }

    private fun awaitOptionalIdle(host: Long) = observer {
        val until = System.nanoTime()+TimeUnit.SECONDS.toNanos(10)
        while (JSONObject(NativeBridge.recognitionStatus(host)).getInt("reserved") != 0) {
            assertTrue("actual native owners did not retire",System.nanoTime()<until);Thread.yield()
        }
    }
    private fun removedReclamationFixture(engines: NotebookEngines): UUID {
        val bytes = "model".toByteArray()
        val sha = java.security.MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it.toInt() and 255) }
        val manifest = JSONObject(descriptor()).put("resource_sha256",sha).toString()
        val staged = observer { engines.importResource(manifest,5,true,null,30000) { _,_ ->
            object: NativeBridge.EngineInput {
                var reads = 0
                override fun read(buffer: ByteArray,control: NativeBridge.EngineControl): Int { control.check();if(reads++>0)return -1;bytes.copyInto(buffer);return bytes.size }
                override fun retire() {}
            }
        }.await() }
        observer { engines.remove(staged.version,30000).await() }
        return staged.version
    }
    @Test fun reclamationReviewRetainsOwnerAcrossConsentAndReturnsImmutableTypedHistory() {
        val host = NativeBridge.open(temporary.newFolder().resolve("library.db").absolutePath)
        val models = temporary.newFolder("reclamation-models");val engines = NotebookEngines(models) { host }
        try {
            val version = removedReclamationFixture(engines)
            assertTrue(File(models,"$version.install.json").delete()) // Explicit legacy/unknown-binding fixture.
            val pending = engines.reviewReclamation(version)
            val review = observer { pending.await() }
            assertEquals(pending.resourceActionId,review.actionId)
            assertTrue(review.summary.facts.bindingReviewRequired)
            assertNull(review.summary.facts.originalImportConsentMillis)
            assertTrue(File(models,"$version.resource").exists())
            assertEquals(NotebookEngines.FailureReason.InvalidInput,failure {review.reclaim(NotebookEngines.ReclamationConsent(true))}.reason)
            val reclaim = review.reclaim(NotebookEngines.ReclamationConsent(true,reviewedUnknownBinding=true))
            val result = observer {reclaim.await()}
            assertEquals(review.actionId,result.id);assertEquals(result.id,reclaim.resourceActionId)
            assertEquals(NotebookEngines.ResourceActionState.Completed,result.state)
            assertFalse(File(models,"$version.resource").exists());assertNull(result.reviews.single().facts.originalImportConsentMillis)
            assertEquals(result,observer {engines.reclamation(result.id).await()})
            val page = observer {engines.reclamations(limit=1).await()};assertEquals(listOf(result),page.actions)
            assertThrows(UnsupportedOperationException::class.java){(page.actions as MutableList).clear()}
            assertThrows(UnsupportedOperationException::class.java){(result.reviews as MutableList).clear()}
            failure {review.reclaim(NotebookEngines.ReclamationConsent(true,true))}
        } finally {engines.close();NativeBridge.close(host);observer {engines.awaitClosed()}}
    }
    @Test fun controllerCloseRevokesAnUnconsumedNativeReviewEvenWithSurvivingPendingAndReviewHandles() {
        val path = temporary.newFolder().resolve("library.db").absolutePath
        var host = NativeBridge.open(path);val models = temporary.newFolder("review-close");val engines = NotebookEngines(models) {host}
        val version = removedReclamationFixture(engines)
        val pending = engines.reviewReclamation(version);val review = observer {pending.await()}
        engines.close();observer {engines.awaitClosed()}
        awaitOptionalIdle(host) // Verify close itself retired the review, before another call can clean it up.
        failure {review.reclaim(NotebookEngines.ReclamationConsent(true))}
        assertTrue(File(models,"$version.resource").exists())
        NativeBridge.close(host);host=NativeBridge.open(path)
        val next = NotebookEngines(models) {host}
        try {
            assertEquals(review.actionId,observer {pending.await()}.actionId)
            assertTrue(observer {next.reclamations().await()}.actions.isEmpty())
            val fresh = observer {next.reviewReclamation(version).await()}
            val done = observer {fresh.reclaim(NotebookEngines.ReclamationConsent(true)).await()}
            assertEquals(NotebookEngines.ResourceActionState.Completed,done.state)
        } finally {review.close();next.close();NativeBridge.close(host);observer {next.awaitClosed()}}
    }
    @Test fun captureRevokesAReviewAndUncertainFailureRetainsItsActionWithoutInventingPendingMetadata() {
        val host = NativeBridge.open(temporary.newFolder().resolve("library.db").absolutePath)
        val models = temporary.newFolder("review-capture");val engines = NotebookEngines(models) {host}
        try {
            val version = removedReclamationFixture(engines)
            val review = observer {engines.reviewReclamation(version).await()}
            val pen = UUID.randomUUID().toString();NativeBridge.setCapturePriority(host,pen,true);NativeBridge.setCapturePriority(host,pen,false)
            val pending = review.reclaim(NotebookEngines.ReclamationConsent(true))
            val error = observer {failure {pending.await()}}
            assertTrue(error.metadataMayHaveChanged);assertEquals(review.actionId,error.resourceActionId)
            assertEquals(review.actionId,pending.resourceActionId)
            assertTrue(File(models,"$version.resource").exists())
            assertTrue(observer {engines.reclamations().await()}.actions.isEmpty())
            val fresh = observer {engines.reviewReclamation(version).await()};fresh.close();awaitOptionalIdle(host)
            failure {fresh.reclaim(NotebookEngines.ReclamationConsent(true))}
        } finally {engines.close();NativeBridge.close(host);observer {engines.awaitClosed()}}
    }

    @Test fun queuedConfirmationCancellationRetiresTheConsumedReviewBeforeItsWorkerCanRun() {
        val host = NativeBridge.open(temporary.newFolder().resolve("library.db").absolutePath)
        val models = temporary.newFolder("queued-review");val engines = NotebookEngines(models) {host}
        val entered = CountDownLatch(1);val release = CountDownLatch(1)
        try {
            val version = removedReclamationFixture(engines)
            val review = observer {engines.reviewReclamation(version).await()}
            val blocker = observer {engines.stageResourceFree(descriptor("blocked"),true).await()}
            val model = object: Model() {
                override fun probe(control: NativeBridge.EngineControl) {entered.countDown();assertTrue(release.await(10,TimeUnit.SECONDS));control.check()}
            }
            val activation = engines.activate(blocker.version,0,30000,factory(model))
            assertTrue(entered.await(10,TimeUnit.SECONDS))
            val queued = review.reclaim(NotebookEngines.ReclamationConsent(true))
            assertEquals(review.actionId,queued.resourceActionId);assertTrue(queued.cancel())
            observer {
                val until = System.nanoTime()+TimeUnit.SECONDS.toNanos(10)
                while(JSONObject(NativeBridge.recognitionStatus(host)).getInt("reserved") != 1) {
                    assertTrue("queued review owner survived cancellation",System.nanoTime()<until);Thread.yield()
                }
            }
            assertTrue(File(models,"$version.resource").exists())
            activation.cancel();release.countDown()
            observer {failure {activation.await()}}
            val error = observer {failure {queued.await()}}
            assertEquals(NotebookEngines.FailureReason.Cancelled,error.reason)
            assertEquals(review.actionId,error.resourceActionId);assertFalse(error.metadataMayHaveChanged)
            assertTrue(observer {engines.reclamations().await()}.actions.isEmpty())
        } finally {release.countDown();engines.close();NativeBridge.close(host);observer {engines.awaitClosed()}}
    }

}
