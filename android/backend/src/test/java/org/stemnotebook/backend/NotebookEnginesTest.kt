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

}
