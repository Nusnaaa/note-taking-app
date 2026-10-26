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
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

@RunWith(JniBackendRunner::class)
@Config(sdk=[35])
class CellCaptureIntegrationTest {
    @get:Rule val temporary=TemporaryFolder()
    private fun platform(root:File)=object:ContextWrapper(RuntimeEnvironment.getApplication()) {override fun getNoBackupFilesDir()=root}
    private fun call(backend:NotebookBackend,command:JSONObject):JSONObject {
        val response=JSONObject(backend.request(JSONObject().put("version",1).put("command",command).toString()).get(10,TimeUnit.SECONDS))
        assertTrue(response.optString("error"),response.getBoolean("ok"));return response.getJSONObject("data")
    }
    private fun table(backend:NotebookBackend):Triple<UUID,UUID,UUID> {
        val note=UUID.fromString(call(backend,JSONObject().put("op","Create").put("kind","Note").put("parent",JSONObject.NULL).put("title","cell fixture")).getString("id"))
        val cell=UUID.randomUUID();val neighbour=UUID.randomUUID()
        val content=JSONObject("""{"type":"Table","value":{"rows":[[{"id":"$cell","value":{"type":"Text","value":"old"}},{"id":"$neighbour","value":{"type":"Numeric","value":"001"}}]],"header_rows":0}}""")
        val block=UUID.fromString(call(backend,JSONObject().put("op","Append").put("entity",note).put("content",content)).getString("id"))
        return Triple(note,block,cell)
    }
    private fun block(backend:NotebookBackend,id:UUID)=call(backend,JSONObject().put("op","Block").put("block",id))
    private fun stroke(session:JournaledCaptureSession,sequence:Long)=CapturedStroke(UUID.randomUUID().toString(),session.id.toString(),sequence,7,false,listOf(ObservedPoint(-0.0,1.25,sequence*10,null,null,null,null)),StrokeEnd.Completed)
    private fun close(backend:NotebookBackend) {backend.closeFullyAsync().get(20,TimeUnit.SECONDS)}
    @Test fun sequentialCellsCaptureAndScopedRecognitionPreserveNeighbours() {
        val backend=NotebookBackend(platform(temporary.newFolder()))
        try {
            val (note,id,cell)=table(backend);val session=backend.openCaptureSession(note,"table-source",cell=CaptureCell(id,cell,0))
            val first=stroke(session,0);val second=stroke(session,1)
            val firstBinding=session.cellBinding(UUID.fromString(first.id));val secondBinding=session.cellBinding(UUID.fromString(second.id))
            assertTrue(backend.checkpointCapture(session,first,1,false,firstBinding).get(10,TimeUnit.SECONDS).committed)
            assertTrue(backend.checkpointCapture(session,second,2,false,secondBinding).get(10,TimeUnit.SECONDS).committed)
            assertTrue(backend.checkpointCapture(session,first,1,false,firstBinding).get(10,TimeUnit.SECONDS).committed)
            assertEquals(2L,block(backend,id).getLong("revision"))
            val worker=Executors.newSingleThreadExecutor()
            try {worker.submit(Callable {
                val engines=backend.engines()
                val descriptor="""{"id":"cell-synthetic","version":"1","adapter_version":"1","family":"Recogniser","input":"OrderedStrokesV1","tasks":["Prose"],"internals":"StrokeOnlyVerified","offline":true,"content_egress":false,"personal_use_evidence":"app-owned synthetic cell fixture","redistributable_resources":false,"resource_sha256":null,"runtime_contract":"stem-processing-v1"}"""
                val version=engines.stageResourceFree(descriptor,true).await().version
                engines.activate(version,0,30000,NativeBridge.RecogniserFactory {_,resource,control ->assertNull(resource);control.check();object:NativeBridge.Recogniser {
                    override fun probe(control:NativeBridge.EngineControl) {control.check()}
                    override fun recognise(ink:String,task:String,control:NativeBridge.EngineControl):String {
                        control.check();val rows=JSONObject(ink).getJSONArray("strokes");assertEquals(2,rows.length())
                        assertEquals(first.id,rows.getJSONObject(0).getString("id"));assertEquals(second.id,rows.getJSONObject(1).getString("id"))
                        return """{"candidates":[{"type":"Text","value":"1 + 1 = 3"}],"raw_output":"1 + 1 = 3","confidence":null,"diagnostics":[]}"""
                    }
                    override fun retire() {}
                }}).await()
                engines.recognise(id,cell,NotebookEngines.Task.Prose,"cell-synthetic",30000).await()
            }).get(30,TimeUnit.SECONDS).also {proposal ->
                val accepted=call(backend,JSONObject().put("op","AcceptProposal").put("proposal",proposal.id).put("candidate",0))
                val cells=accepted.getJSONObject("content").getJSONObject("value").getJSONArray("rows").getJSONArray(0)
                assertEquals("1 + 1 = 3",cells.getJSONObject(0).getJSONObject("value").getString("value"));assertEquals("001",cells.getJSONObject(1).getJSONObject("value").getString("value"))
            }} finally {worker.shutdownNow()}
        } finally {close(backend)}
    }
    @Test fun staleCellCheckpointSurvivesReopenAndRequiresExplicitFreshRetarget() {
        val root=temporary.newFolder();var backend=NotebookBackend(platform(root));val ids=table(backend);val (note,id,cell)=ids
        try {
            val session=backend.openCaptureSession(note,"table-source",cell=CaptureCell(id,cell,0));val original=stroke(session,0)
            call(backend,JSONObject().put("op","Edit").put("block",id).put("revision",0).put("content",block(backend,id).getJSONObject("content")))
            val receipt=backend.checkpointCapture(session,original,1,false).get(10,TimeUnit.SECONDS);assertTrue(receipt.checkpointed);assertFalse(receipt.committed)
            close(backend);backend=NotebookBackend(platform(root))
            val review=backend.reviewInterruptedCapture(receipt.ticket).get(10,TimeUnit.SECONDS)
            assertEquals(CaptureCell(id,cell,0),review.cell);assertEquals(original.points,review.stroke.points);assertEquals(false,review.cellAvailable)
            val retarget=backend.reviewCaptureCellRetarget(review,CaptureCell(id,cell,1)).get(10,TimeUnit.SECONDS)
            call(backend,JSONObject().put("op","Edit").put("block",id).put("revision",1).put("content",block(backend,id).getJSONObject("content")))
            assertThrows(java.util.concurrent.ExecutionException::class.java) {backend.confirmCaptureCellRetarget(retarget,true).get(10,TimeUnit.SECONDS)}
            val fresh=backend.reviewCaptureCellRetarget(review,CaptureCell(id,cell,2)).get(10,TimeUnit.SECONDS)
            val committed=backend.confirmCaptureCellRetarget(fresh,true).get(10,TimeUnit.SECONDS);assertTrue(committed.checkpointed);assertTrue(committed.committed)
            assertEquals(3L,block(backend,id).getLong("revision"));assertEquals(0,backend.captureRecoveryState().get(10,TimeUnit.SECONDS).pending)
        } finally {close(backend)}
    }
    @Test fun incorrectOwnerCannotPublishAValidForeignTableTarget() {
        val backend=NotebookBackend(platform(temporary.newFolder()))
        try {
            val (_,id,cell)=table(backend);val (other,_,_)=table(backend)
            val session=backend.openCaptureSession(other,"table-source",cell=CaptureCell(id,cell,0))
            val receipt=backend.checkpointCapture(session,stroke(session,0),1,false).get(10,TimeUnit.SECONDS)
            assertTrue(receipt.checkpointed);assertFalse(receipt.committed);assertEquals(0L,block(backend,id).getLong("revision"))
        } finally {close(backend)}
    }
    @Test fun completedContactsReleaseLookupCapacityWhileCallerRetryBindingSurvives() {
        val backend=NotebookBackend(platform(temporary.newFolder()))
        try {
            val (note,id,cell)=table(backend);val session=backend.openCaptureSession(note,"table-source",cell=CaptureCell(id,cell,0))
            val first=stroke(session,0);val binding=session.cellBinding(UUID.fromString(first.id))
            assertTrue(backend.checkpointCapture(session,first,1,false,binding).get(10,TimeUnit.SECONDS).committed)
            for (sequence in 1L..260L) assertTrue(backend.checkpointCapture(session,stroke(session,sequence),sequence+1,false).get(10,TimeUnit.SECONDS).committed)
            assertTrue(backend.checkpointCapture(session,first,1,false,binding).get(10,TimeUnit.SECONDS).committed)
            assertEquals(261L,block(backend,id).getLong("revision"))
        } finally {close(backend)}
    }

}
