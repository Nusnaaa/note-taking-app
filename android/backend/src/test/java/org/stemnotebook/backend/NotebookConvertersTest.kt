package org.stemnotebook.backend

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import java.util.UUID
import java.util.concurrent.*
import java.util.concurrent.atomic.AtomicInteger

@RunWith(JniBackendRunner::class)
@Config(sdk = [35])
class NotebookConvertersTest {
    @get:Rule val temporary = TemporaryFolder()
    private fun <T> observer(action: () -> T): T {
        val worker = Executors.newSingleThreadExecutor()
        return try {worker.submit(Callable {action()}).get(30,TimeUnit.SECONDS)} finally {worker.shutdownNow()}
    }
    private fun command(host: Long, value: JSONObject): JSONObject {
        val response = JSONObject(NativeBridge.request(host,JSONObject().put("version",1).put("command",value).toString()))
        assertTrue(response.optString("error"),response.getBoolean("ok"));return response.getJSONObject("data")
    }
    private fun descriptor() = """{"id":"synthetic-kotlin-converter","version":"1","adapter_version":"1","family":"Converter","input":"EditableV1","tasks":[],"internals":"StrokeOnlyVerified","offline":true,"content_egress":false,"personal_use_evidence":"app-owned Kotlin converter fixture","redistributable_resources":false,"resource_sha256":null,"runtime_contract":"stem-processing-v1"}"""
    private fun source(host: Long): UUID {
        val note = command(host,JSONObject().put("op","Create").put("kind","Note").put("title","converter").put("parent",JSONObject.NULL)).getString("id")
        return UUID.fromString(command(host,JSONObject().put("op","Append").put("entity",note)
            .put("content",JSONObject().put("type","Code").put("value",JSONObject().put("text","\t001\r\n1+1=3").put("language",JSONObject.NULL)))).getString("id"))
    }
    private open class Converter : NativeBridge.Converter {
        val retired = AtomicInteger()
        override fun probe(control: NativeBridge.EngineControl) {assertEquals("stem-engines",Thread.currentThread().name);control.check()}
        override fun formats(control: NativeBridge.EngineControl): String {control.check();return """["PlainText"]"""}
        override fun convert(content: String,target: String,control: NativeBridge.EngineControl): String {
            assertEquals("stem-engines",Thread.currentThread().name);control.check();assertEquals("PlainText",target)
            assertEquals("\t001\r\n1+1=3",JSONObject(content).getJSONObject("value").getString("text"))
            return """{"text":"\t001\r\n1+1=3","fidelity":{"complete":false,"lexical_change":false,"layout_change":true,"unsupported":["synthetic layout warning"],"semantic_equivalence_asserted":false}}"""
        }
        override fun retire() {assertEquals(1,retired.incrementAndGet())}
    }
    private fun activate(engines: NotebookEngines, model: Converter) {
        val version = engines.stageResourceFree(descriptor(),true).await().version
        engines.activateConverter(version,0,30000,NativeBridge.ConverterFactory { _,resource,control ->assertNull(resource);control.check();model}).await()
    }
    @Test fun typedDerivativeRetainsSourceAndFidelityWithoutMutation() {
        val host = NativeBridge.open(temporary.newFolder().resolve("library.db").absolutePath)
        val engines = NotebookEngines {host};val model = Converter()
        try {observer {
            activate(engines,model);val block = source(host)
            val result = engines.convert(block,null,"synthetic-kotlin-converter",NotebookEngines.ConversionFormat.PlainText,30000).await()
            assertEquals(block,result.block);assertNull(result.region);assertEquals(0L,result.revision)
            assertEquals("\t001\r\n1+1=3",result.text);assertFalse(result.fidelity.complete)
            assertFalse(result.fidelity.lexicalChange);assertTrue(result.fidelity.layoutChange)
            assertEquals(listOf("synthetic layout warning"),result.fidelity.unsupported);assertFalse(result.fidelity.semanticEquivalenceAsserted)
            assertThrows(UnsupportedOperationException::class.java) {(result.fidelity.unsupported as MutableList<String>).add("rewritten")}
            val stored = command(host,JSONObject().put("op","Block").put("block",block))
            assertEquals(0L,stored.getLong("revision"));assertEquals(result.text,stored.getJSONObject("content").getJSONObject("value").getString("text"))
        }} finally {engines.close();NativeBridge.close(host)}
        assertEquals(1,model.retired.get())
    }
    @Test fun boundedControllerCancelsRunningAndQueuedConversionsWithoutMetadataChanges() {
        val host = NativeBridge.open(temporary.newFolder().resolve("library.db").absolutePath)
        val entered = CountDownLatch(1);val release = CountDownLatch(1)
        val engines = NotebookEngines {host}
        val model = object : Converter() {
            override fun convert(content: String,target: String,control: NativeBridge.EngineControl): String {
                entered.countDown();assertTrue(release.await(10,TimeUnit.SECONDS));control.check();return super.convert(content,target,control)
            }
        }
        try {observer {
            activate(engines,model);val block = source(host)
            val running = engines.convert(block,null,"synthetic-kotlin-converter",NotebookEngines.ConversionFormat.PlainText,30000)
            assertTrue(entered.await(10,TimeUnit.SECONDS))
            val queued = engines.convert(block,null,"synthetic-kotlin-converter",NotebookEngines.ConversionFormat.PlainText,30000)
            assertEquals(NotebookEngines.FailureReason.Busy,assertThrows(NotebookEngines.Failure::class.java) {engines.convert(block,null,"synthetic-kotlin-converter",NotebookEngines.ConversionFormat.PlainText,30000)}.reason)
            queued.cancel();running.cancel();release.countDown()
            for (pending in listOf(running,queued)) {
                val error = assertThrows(NotebookEngines.Failure::class.java) {pending.await()}
                assertEquals(NotebookEngines.FailureReason.Cancelled,error.reason);assertFalse(error.metadataMayHaveChanged)
            }
            assertEquals(0,JSONObject(NativeBridge.recognitionStatus(host)).getInt("reserved"))
        }} finally {release.countDown();engines.close();NativeBridge.close(host)}
        assertEquals(1,model.retired.get())
    }
    @Test fun converterOwnersUseJniParentLoader() {
        for (type in listOf(NativeBridge.EngineConversion::class.java,NativeBridge.Converter::class.java,NativeBridge.ConverterFactory::class.java)) {
            assertSame(NativeBridge::class.java.classLoader,type.classLoader)
        }
    }
}
