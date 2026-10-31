package org.stemnotebook.backend

import android.content.Intent
import android.net.Uri
import android.os.CancellationSignal
import android.os.Looper
import android.os.ParcelFileDescriptor
import android.provider.DocumentsContract
import org.json.JSONObject
import org.json.JSONArray
import java.io.Closeable
import java.io.File
import java.security.MessageDigest
import java.util.Collections
import java.util.UUID
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.CancellationException
import java.util.concurrent.CompletableFuture
import java.util.concurrent.ExecutionException
import java.util.concurrent.ScheduledThreadPoolExecutor
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit

/** Reviewed native/derivative output, deliberate provider publication and durable
 * attempt discovery. Provider calls and verification never use the writer/UI. */
class NotebookExports internal constructor(directory: File,private val nativeHandle: () -> Long,
    private val documents: ExportDocuments,private val fault: (String) -> Unit = {},private val admission: () -> Unit = {}) : Closeable {
    enum class Format { Native, Markdown, Pdf }
    enum class Outcome { Uncertain, ProviderVerified }
    enum class FailureReason { Unavailable, Cancelled, Busy, Closed, InvalidInput }
    class Failure(val reason: FailureReason,val operationId: UUID? = null,val publicationMayHaveCommitted: Boolean = false) :
        IllegalStateException("Notebook export unavailable")
    data class Summary(val format: Format,val selectedEntities: Long?,val selectedBlocks: Long,val includesOriginalInk: Boolean,
        val bytes: Long?,val warnings: List<String>,val omissions: List<String>)
    data class Part(val name: String,val bytes: Long,val sha256: String,val document: Uri?)
    data class Receipt(val id: UUID,val library: UUID,val format: Format,val tree: Uri,val folder: Uri?,val parts: List<Part>,val outcome: Outcome,val descriptorSynced: Boolean)
    data class Reconciliation(val receipt: Receipt?)
    /** Past recorded attempt headers; reading a page never contacts providers. */
    data class Attempt(val sequence: Long,val id: UUID,val format: Format,val createdMillis: Long,val tree: Uri,val folder: Uri?,val partCount: Int,val outcome: Outcome,val descriptorSynced: Boolean)
    data class History(val attempts: List<Attempt>,val next: Long?)
    class Publication internal constructor(val receipt: Receipt,val catalogueRecorded: Boolean,private val key: CharArray?) : Closeable {
        private var closed=false
        @Synchronized fun copyRecoveryKey(): CharArray? { check(!closed);return key?.clone() }
        @Synchronized override fun close() { if(!closed) { closed=true;key?.fill('\u0000') } }
    }
    class Pending<T : Any> internal constructor(private val result: CompletableFuture<T>,private val cancelAction: () -> Boolean,
        private val observer: () -> Unit,private val identity: () -> UUID?,private val attempted: () -> Boolean) : Closeable {
        val operationId: UUID? get()=identity()
        val isReady: Boolean get()=result.isDone
        fun await(): T {
            observer()
            return try { result.get() }
            catch(error: InterruptedException) { cancel();Thread.currentThread().interrupt();throw Failure(FailureReason.Cancelled,operationId,attempted()) }
            catch(error: ExecutionException) { throw error.cause as? Failure ?: Failure(FailureReason.Unavailable,operationId,attempted()) }
        }
        fun cancel(): Boolean=cancelAction()
        override fun close() { cancel();result.whenComplete { value,_ -> (value as? AutoCloseable)?.close() } }
    }
    inner class Review internal constructor(internal val native: AutoCloseable,val id: UUID,internal val library: UUID,val summary: Summary) : Closeable {
        private var closed=false
        private var claimed=false
        internal var destination: Destination?=null
        internal fun checkLive() { synchronized(lock) { if(closed || claimed || this !in reviews) throw Failure(FailureReason.InvalidInput,id) } }
        internal fun claim(value: Destination) { synchronized(lock) { checkLive();if(destination!==value) throw Failure(FailureReason.InvalidInput,id);claimed=true } }
        fun destination(tree: Uri,pickerFlags: Int,timeoutMillis: Long=30000): Pending<Destination> = selectDestination(this,tree,pickerFlags,timeoutMillis)
        override fun close() {
            val release=synchronized(lock) { if(closed) false else { closed=true;reviews.remove(this);true } }
            if(release) native.close()
        }
    }
    inner class Destination internal constructor(internal val review: Review,internal val tree: Uri,internal val observed: ExportDocuments.Selection) : Closeable {
        val operationId: UUID get()=review.id
        val label: String get()=observed.label
        fun publish(proof: NativeBridge.Verification,confirmed: Boolean,acknowledgeWarnings: Boolean,
            acknowledgeProtectedPlaintext: Boolean=false,password: CharArray?=null,timeoutMillis: Long=120000): Pending<Publication> =
            publishDestination(this,proof,confirmed,acknowledgeWarnings,acknowledgeProtectedPlaintext,password,timeoutMillis)
        override fun close() { review.close() }
    }
    private val catalogue=ExportCatalogue(directory)
    private val lock = Any()
    private var closed = false
    private val operations = mutableSetOf<Operation<*>>()
    private val reviews = mutableSetOf<Review>()
    @Volatile private var workerThread: Thread? = null
    private val worker = ThreadPoolExecutor(1, 1, 0, TimeUnit.MILLISECONDS, ArrayBlockingQueue(1),
        { action -> Thread(action, "stem-exports").apply { isDaemon = true; workerThread = this } }, ThreadPoolExecutor.AbortPolicy())
    // At most two admitted operations queue cancellation once each. Admission
    // remains held until their cancellation and worker actually retire.
    private val cancellations = ThreadPoolExecutor(1, 1, 0, TimeUnit.MILLISECONDS, ArrayBlockingQueue(2),
        { action -> Thread(action, "stem-export-cancel").apply { isDaemon = true } }, ThreadPoolExecutor.AbortPolicy())
    private val deadlines = ScheduledThreadPoolExecutor(1) { action -> Thread(action, "stem-export-deadline").apply { isDaemon = true } }
        .apply { removeOnCancelPolicy = true }
    private inner class Operation<T : Any>(val timeoutMillis: Long, val reviewResult: Boolean) {
        val result = CompletableFuture<T>()
        private val state = Any()
        private var cancelled = false
        private var finished = false
        private var retiring = false
        private var resource: AutoCloseable? = null
        private var signal: CancellationSignal? = null
        private var input: AutoCloseable? = null
        private var cancellation: CompletableFuture<Void>? = null
        private val started = System.nanoTime()
        @Volatile var identity: UUID? = null
        @Volatile var attempted = false
        fun remaining(): Long {
            check()
            return (timeoutMillis - TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - started)).coerceAtLeast(1)
        }
        fun check() { synchronized(state) {
            if (cancelled || System.nanoTime() - started >= TimeUnit.MILLISECONDS.toNanos(timeoutMillis))
                throw Failure(FailureReason.Cancelled, identity, attempted)
        } }
        fun attach(value: AutoCloseable) {
            val reject = synchronized(state) { if (cancelled) true else { resource = value; false } }
            if (reject) { value.close(); throw Failure(FailureReason.Cancelled, identity, attempted) }
        }
        fun attach(value: CancellationSignal) {
            val reject = synchronized(state) { if (cancelled) true else { signal = value; false } }
            if (reject) { value.cancel(); throw Failure(FailureReason.Cancelled, identity, attempted) }
        }
        fun attachIo(value: AutoCloseable) {
            val reject = synchronized(state) { if (cancelled) true else { input = value; false } }
            if (reject) { value.close(); throw Failure(FailureReason.Cancelled, identity, attempted) }
        }
        fun clearInput() { synchronized(state) { input = null; signal = null } }
        fun cancel(): Boolean {
            val values = synchronized(state) {
                if (finished || cancelled) return false
                cancelled = true
                if (retiring) return true
                val done = CompletableFuture<Void>(); cancellation = done
                Triple(resource, signal, input) to done
            }
            cancellations.execute {
                try {
                    try { values.first.first?.close() }
                    finally { try { values.first.second?.cancel() } finally { values.first.third?.close() } }
                } catch (_: Throwable) { /* generic failure; retirement still waits for the worker */ }
                finally { values.second.complete(null) }
            }
            return true
        }
        fun retire() { synchronized(state) { retiring = true; cancellation }?.join() }
        fun complete(value: T?, error: Throwable?) {
            val failure = synchronized(state) {
                finished = true
                resource = null; signal = null; input = null
                if (error == null && !(reviewResult && cancelled)) null
                else if (error is Failure) Failure(error.reason, error.operationId ?: identity, error.publicationMayHaveCommitted || attempted)
                else Failure(if (cancelled || error is CancellationException) FailureReason.Cancelled else FailureReason.Unavailable, identity, attempted)
            }
            if (failure == null) result.complete(requireNotNull(value)) else {
                (value as? AutoCloseable)?.close()
                result.completeExceptionally(failure)
            }
        }
    }
    private fun observerOnly() {
        if (Thread.currentThread() === workerThread || Looper.myLooper() === Looper.getMainLooper()) throw Failure(FailureReason.Unavailable)
    }
    private fun <T : Any> submit(timeout: Long, reviewResult: Boolean = false, initial: Review? = null,
        cleanup: () -> Unit = {}, action: (Operation<T>) -> T): Pending<T> {
        val operation = Operation<T>(timeout, reviewResult)
        operation.identity = initial?.id
        if (initial != null) operation.attach(initial)
        try { synchronized(lock) {
            if (closed) throw Failure(FailureReason.Closed, operation.identity)
            if (operations.size >= 2) throw Failure(FailureReason.Busy, operation.identity)
            operations.add(operation)
            try { worker.execute {
                var value: T? = null; var error: Throwable? = null
                var deadline: java.util.concurrent.ScheduledFuture<*>? = null
                try {
                    operation.check()
                    deadline = deadlines.schedule({ operation.cancel() }, operation.remaining(), TimeUnit.MILLISECONDS)
                    value = action(operation)
                } catch (failure: Throwable) { error = failure }
                finally {
                    deadline?.cancel(false)
                    try { operation.retire(); initial?.close(); cleanup() }
                    catch (failure: Throwable) { if (error == null) error = failure }
                    operation.complete(value, error)
                    synchronized(lock) { operations.remove(operation) }
                }
            } } catch (error: Throwable) { operations.remove(operation); throw error }
        } } catch (error: Throwable) {
            initial?.close(); cleanup()
            throw error as? Failure ?: Failure(FailureReason.Unavailable, operation.identity)
        }
        return Pending(operation.result, operation::cancel, ::observerOnly, { operation.identity }, { operation.attempted })
    }
    private fun budget(value: Long) { if(value !in 1..3_600_000) throw Failure(FailureReason.InvalidInput) }
    fun reviewNative(selection: String,originalInk: Boolean,timeoutMillis: Long=30000): Pending<Review> = review(Format.Native,selection,originalInk,null,null,timeoutMillis)
    fun reviewMarkdown(selection: String,originalInk: Boolean,timeoutMillis: Long=30000): Pending<Review> = review(Format.Markdown,selection,originalInk,null,null,timeoutMillis)
    fun reviewPdf(selection: String,font: ByteArray,page: String,timeoutMillis: Long=30000): Pending<Review> = review(Format.Pdf,selection,false,font,page,timeoutMillis)
    private fun review(format: Format,selection: String,ink: Boolean,font: ByteArray?,page: String?,timeout: Long): Pending<Review> {
        budget(timeout)
        if(selection.length>1024*1024 || font!=null && font.size>32*1024*1024 || page!=null && page.length>4096) throw Failure(FailureReason.InvalidInput)
        val copy=font?.clone()
        return submit(timeout,true,cleanup={ copy?.fill(0) }) { operation ->
            admission();operation.check();val host=nativeHandle()
            val library=UUID.fromString(NativeBridge.captureLibraryIdentity(host))
            val job: AutoCloseable=if(format==Format.Native) NativeBridge.reviewNativeExport(host,selection,ink) else NativeBridge.reviewDerivativeExport(host,selection,ink)
            var retained: Review?=null
            try {
                operation.attach(job)
                val row=JSONObject(when(job) {
                    is NativeBridge.NativeExport -> job.summary
                    is NativeBridge.DerivativeExport -> if(format==Format.Markdown) job.renderMarkdown(operation.remaining()) else job.renderPdf(copy,page,operation.remaining())
                    else -> throw Failure(FailureReason.Unavailable)
                })
                val summary=if(format==Format.Native) Summary(format,row.getLong("selected_entities"),row.getLong("selected_blocks"),row.getBoolean("includes_raw_ink"),null,emptyList(),strings(row.getJSONArray("omissions")))
                    else Summary(format,null,row.getLong("selected_blocks"),row.getBoolean("includes_original_ink"),row.getLong("bytes"),strings(row.getJSONArray("warnings")),emptyList())
                val value=Review(job,UUID.randomUUID(),library,summary);retained=value;operation.identity=value.id
                synchronized(lock) { if(closed) throw Failure(FailureReason.Closed,value.id);reviews.add(value) }
                operation.attach(value);operation.check();value
            } catch(error: Throwable) { retained?.close() ?: job.close();throw error }
        }
    }
    private fun selectDestination(review: Review,tree: Uri,flags: Int,timeout: Long): Pending<Destination> {
        budget(timeout)
        val access=Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
        if(tree.scheme!="content" || tree.toString().length>4096 || !DocumentsContract.isTreeUri(tree) || flags and access!=access) throw Failure(FailureReason.InvalidInput,review.id)
        review.checkLive()
        return submit(timeout,true) { operation ->
            operation.identity=review.id;operation.attach(review)
            try {
                admission();val signal=CancellationSignal();operation.attach(signal)
                val observed=documents.inspect(tree,signal);operation.check()
                require(observed.identity==DocumentsContract.getTreeDocumentId(tree) && observed.label.length<=4096)
                val value=Destination(review,tree,observed)
                synchronized(lock) { review.checkLive();require(review.destination==null);review.destination=value }
                value
            } catch(error: Throwable) { review.close();throw error }
        }
    }
    private data class Material(val parts: LinkedHashMap<String,ByteArray>,val key: CharArray?,val close: () -> Unit)
    private fun finish(review: Review,password: CharArray?,proof: NativeBridge.Verification,ack: Boolean,protected: Boolean,remaining: Long): Material = when(val job=review.native) {
        is NativeBridge.NativeExport -> {
            val data=job.finish(password,proof,protected,ack,remaining)
            Material(linkedMapOf((if(password==null) "document.stem.json" else "document.stemenc") to data.bytes),data.recoveryKey,data::close)
        }
        is NativeBridge.DerivativeExport -> {
            val data=job.finish(proof,true)
            try {
                val parts=linkedMapOf<String,ByteArray>()
                if(data.kind=="Pdf") parts["document.pdf"]=requireNotNull(data.pdf)
                else {
                    require(data.kind=="Markdown" && data.assetNames.size==data.assets.size)
                    parts["document.md"]=requireNotNull(data.markdown)
                    data.assetNames.zip(data.assets).forEach { (name,bytes) -> require(name !in parts);parts[name]=bytes }
                }
                Material(parts,null,data::close)
            } catch(error: Throwable) { data.close();throw error }
        }
        else -> throw Failure(FailureReason.Unavailable)
    }
    private fun publishDestination(selected: Destination,proof: NativeBridge.Verification,confirmed: Boolean,ack: Boolean,protected: Boolean,password: CharArray?,timeout: Long): Pending<Publication> {
        budget(timeout);val review=selected.review
        if(!confirmed || (review.summary.warnings.isNotEmpty() || review.summary.omissions.isNotEmpty()) && !ack ||
            password!=null && (review.summary.format!=Format.Native || password.size>1024)) throw Failure(FailureReason.InvalidInput,review.id)
        review.claim(selected);val secret=password?.clone()
        return submit(timeout,initial=review,cleanup={ secret?.fill('\u0000') }) { operation ->
            admission();val signal=CancellationSignal();operation.attach(signal)
            fun current() { operation.check();require(documents.inspect(selected.tree,signal)==selected.observed) }
            current()
            val material=finish(review,secret,NativeBridge.Verification { challenge -> current();val accepted=proof.verify(challenge);current();accepted },ack,protected,operation.remaining())
            secret?.fill('\u0000') // Provider I/O no longer needs the owned credential copy.
            var key: CharArray?=null;var delivered=false
            try {
                current();key=material.key?.clone()
                val payload=material.parts
                require(payload.keys.none { it=="STEM-export.json" } && payload.keys.all { it.matches(Regex("[A-Za-z0-9][A-Za-z0-9._-]{0,159}")) })
                require(payload.values.sumOf { it.size.toLong() }<=64L*1024*1024+208)
                val parts=payload.map { (name,bytes) -> operation.check();ExportCatalogue.Part(name,bytes.size.toLong(),digest(bytes)) }
                val manifest=JSONObject().put("format",1).put("operation",review.id.toString()).put("kind",review.summary.format.name)
                    .put("includes_original_ink",review.summary.includesOriginalInk).put("warnings",JSONArray(review.summary.warnings)).put("omissions",JSONArray(review.summary.omissions))
                    .put("files",JSONArray(parts.map { p -> JSONObject().put("name",p.name).put("bytes",p.bytes).put("sha256",p.sha256) })).toString().toByteArray(Charsets.UTF_8)
                require(manifest.size<=8*1024*1024)
                payload["STEM-export.json"]=manifest
                var record=ExportCatalogue.Record(review.id,review.library,review.summary.format.name,System.currentTimeMillis().coerceAtLeast(0),selected.tree,null,
                    parts+ExportCatalogue.Part("STEM-export.json",manifest.size.toLong(),digest(manifest)),ExportCatalogue.State.Uncertain,false)
                catalogue.write(record,operation::check);current()
                try {
                    operation.attempted=true
                    val folder=documents.create(selected.observed.root,"STEM-export-${review.id}",DocumentsContract.Document.MIME_TYPE_DIR)
                    validateUri(folder,selected.tree);require(folder!=selected.observed.root && documents.isChild(selected.observed.root,folder))
                    record=record.copy(folder=folder);catalogue.setFolder(record.id,record.library,folder,operation::check)
                    require(documents.name(folder,signal)=="STEM-export-${review.id}")
                    var synced=true
                    val mutableParts=record.parts.toMutableList();record=record.copy(parts=mutableParts)
                    val knownDocuments=HashSet<Uri>()
                    for((index,part) in mutableParts.withIndex()) {
                        current()
                        val uri=documents.create(folder,part.name,mime(part.name));validateUri(uri,selected.tree)
                        require(uri!=folder && uri!=selected.observed.root && uri !in knownDocuments && documents.isChild(folder,uri))
                        knownDocuments.add(uri);mutableParts[index]=part.copy(document=uri)
                        catalogue.setPart(record.id,record.library,index,uri,operation::check)
                        require(documents.name(uri,signal)==part.name)
                        // Never open a nonempty returned document for overwrite.
                        val empty=documents.open(uri,false,signal);operation.attachIo(empty)
                        try { ParcelFileDescriptor.AutoCloseInputStream(empty).use { input -> require(input.read()==-1);empty.checkError() } } finally { empty.close() }
                        val descriptor=documents.open(uri,true,signal);operation.attachIo(descriptor)
                        try {
                            ParcelFileDescriptor.AutoCloseOutputStream(descriptor).use { output ->
                                val bytes=requireNotNull(payload[part.name]);var at=0
                                while(at<bytes.size) { operation.check();val n=minOf(32768,bytes.size-at);output.write(bytes,at,n);at+=n }
                                operation.check();output.flush();descriptor.checkError()
                                try { descriptor.fileDescriptor.sync() } catch(_: java.io.SyncFailedException) { synced=false }
                            }
                        } finally { descriptor.close() }
                        fault("provider-written")
                        val read=documents.open(uri,false,signal);operation.attachIo(read)
                        try {
                            ParcelFileDescriptor.AutoCloseInputStream(read).use { input ->
                                val hash=MessageDigest.getInstance("SHA-256");val buffer=ByteArray(32768);var total=0L
                                try { while(true) { operation.check();val n=input.read(buffer);if(n<0) break;require(n>0 && n<=buffer.size && n.toLong()<=part.bytes-total);hash.update(buffer,0,n);total+=n } }
                                finally { buffer.fill(0) }
                                read.checkError();require(total==part.bytes && hex(hash.digest())==part.sha256)
                            }
                        } finally { read.close() }
                        current()
                    }
                    record=record.copy(state=ExportCatalogue.State.ProviderVerified,descriptorSynced=synced)
                    val recorded=try { catalogue.verified(record.id,record.library,synced);true } catch(_: Exception) { false }
                    Publication(receipt(record),recorded,key).also { delivered=true }
                } catch(_: Throwable) {
                    val recorded=try { catalogue.write(record);true } catch(_: Exception) { false }
                    Publication(receipt(record),recorded,key).also { delivered=true }
                }
            } finally { if(!delivered) key?.fill('\u0000');material.parts["STEM-export.json"]?.fill(0);material.close() }
        }
    }
    fun reconcile(id: UUID): Pending<Reconciliation> = submit(30000) { operation ->
        operation.identity=id;val host=nativeHandle();NativeBridge.backupGeneration(host);operation.check()
        Reconciliation(catalogue.lookup(id,UUID.fromString(NativeBridge.captureLibraryIdentity(host)),operation::check)?.let(::receipt))
    }
    /** Discover attempts after a restart without retaining their UUIDs. A cursor
     * is the last returned sequence; later insertions do not shift older pages. */
    fun history(before: Long?=null,limit: Int=25): Pending<History> {
        if(limit !in 1..100 || before!=null && before<=0) throw Failure(FailureReason.InvalidInput)
        return submit(30000) { operation ->
            val host=nativeHandle();NativeBridge.backupGeneration(host);operation.check()
            val page=catalogue.history(UUID.fromString(NativeBridge.captureLibraryIdentity(host)),before,limit,operation::check)
            History(Collections.unmodifiableList(page.attempts.map { a -> Attempt(a.sequence,a.id,Format.valueOf(a.format),a.createdMillis,a.tree,a.folder,a.partCount,Outcome.valueOf(a.state.name),a.descriptorSynced) }),page.next)
        }
    }
    /** Retire optional input and idle reviews, preserving committed receipts. */
    fun invalidate() {
        val values = synchronized(lock) { operations.toList() to reviews.toList() }
        values.first.forEach { it.cancel() }; values.second.forEach { it.close() }
    }
    override fun close() {
        synchronized(lock) { if (closed) return; closed = true }
        invalidate(); worker.shutdown()
    }
    fun awaitClosed() {
        observerOnly()
        while (!worker.awaitTermination(1, TimeUnit.DAYS)) { }
        deadlines.shutdown()
        while (!deadlines.awaitTermination(1, TimeUnit.DAYS)) { }
        cancellations.shutdown()
        while (!cancellations.awaitTermination(1, TimeUnit.DAYS)) { }
    }
    companion object {
        private fun strings(array: JSONArray): List<String> = Collections.unmodifiableList((0 until array.length()).map { array.getString(it) })
        private fun hex(bytes: ByteArray)=bytes.joinToString("") { "%02x".format(it.toInt() and 255) }
        private fun digest(bytes: ByteArray)=hex(MessageDigest.getInstance("SHA-256").digest(bytes))
        private fun validateUri(uri: Uri,tree: Uri) { require(uri.scheme=="content" && uri.authority==tree.authority && uri.toString().length<=4096 && DocumentsContract.getTreeDocumentId(uri)==DocumentsContract.getTreeDocumentId(tree)) }
        private fun mime(name: String)=when { name.endsWith(".pdf") -> "application/pdf";name.endsWith(".md") -> "text/markdown";name.endsWith(".svg") -> "image/svg+xml";name.endsWith(".png") -> "image/png";name.endsWith(".jpg") || name.endsWith(".jpeg") -> "image/jpeg";name.endsWith(".json") -> "application/json";else -> "application/octet-stream" }
        private fun receipt(r: ExportCatalogue.Record)=Receipt(r.id,r.library,Format.valueOf(r.format),r.tree,r.folder,
            Collections.unmodifiableList(r.parts.map { Part(it.name,it.bytes,it.sha256,it.document) }),Outcome.valueOf(r.state.name),r.descriptorSynced)
    }
}
