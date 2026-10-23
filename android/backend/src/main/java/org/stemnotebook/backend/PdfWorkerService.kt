package org.stemnotebook.backend

import android.app.Service
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Color
import android.graphics.pdf.PdfRenderer
import android.os.*
import java.io.IOException
import java.util.concurrent.atomic.AtomicBoolean

/** OS-isolated parser receives an explicit read-only descriptor, not a notebook
 * path, credentials, network permission or source-retention authority. */
class PdfWorkerService : Service() {
    private lateinit var thread: HandlerThread
    private val busy = AtomicBoolean(false)
    override fun onCreate() { super.onCreate(); thread = HandlerThread("pdf-render").apply { start() } }
    override fun onBind(intent: Intent): IBinder = Messenger(Handler(Looper.getMainLooper()) { message ->
        receive(message)
        true
    }).binder
    @Suppress("DEPRECATION")
    private fun receive(message: Message) {
        var descriptor: ParcelFileDescriptor? = null
        var admitted = false
        var ownsSlot = false
        try {
            descriptor = message.data.getParcelable("descriptor")
            // Control stays on the main looper while the parser/writer may block.
            if (message.what == 99) {
                descriptor?.close()
                descriptor = null
                Process.killProcess(Process.myPid())
                return
            }
            val output = message.replyTo ?: return
            val fd = descriptor ?: throw IOException("PDF descriptor missing")
            val job = PdfRenderJob(message.what, message.arg2, message.arg1, message.data.getInt("width"), message.data.getInt("height"), fd)
            if (!busy.compareAndSet(false, true)) { sendFailure(output, message.what, message.arg2); return }
            ownsSlot = true
            admitted = Handler(thread.looper).post {
                try { job.render { output.send(it) } } finally { busy.set(false) }
            }
            if (!admitted) { busy.set(false); sendFailure(output, message.what, message.arg2) }
        } catch (_: Exception) {
            message.replyTo?.let { sendFailure(it, message.what, message.arg2) }
        } finally {
            // Ownership passes to the worker only after successful Handler admission.
            if (!admitted) {
                if (ownsSlot) busy.set(false)
                try { descriptor?.close() } catch (_: IOException) { }
            }
        }
    }
    private fun sendFailure(output: Messenger, what: Int, token: Int) {
        try { output.send(Message.obtain(null, what).apply { arg2 = token; data = Bundle().apply { putBoolean("ok", false) } }) } catch (_: Exception) { }
    }
    override fun onDestroy() { thread.quitSafely(); super.onDestroy() }
}

/** Small owned parser job: the descriptor closes even before PdfRenderer exists. */
internal data class PdfRenderJob(val what: Int, val token: Int, val page: Int, val width: Int, val height: Int, val descriptor: ParcelFileDescriptor) {
    fun render(send: (Message) -> Unit) {
        var replied = false
        try {
            descriptor.use { fd ->
                require(width in 1..4096 && height in 1..4096 && width.toLong() * height <= 8_000_000)
                PdfRenderer(fd).use { renderer ->
                    require(page in 0 until renderer.pageCount)
                    renderer.openPage(page).use { source ->
                        val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
                        try {
                            bitmap.eraseColor(Color.WHITE)
                            source.render(bitmap, null, null, PdfRenderer.Page.RENDER_MODE_FOR_DISPLAY)
                            streamPdfPixels(bitmap) { input ->
                                send(Message.obtain(null, what).apply {
                                    arg2 = token
                                    data = Bundle().apply { putParcelable("pixels", input); putInt("pages", renderer.pageCount); putBoolean("ok", true) }
                                })
                                replied = true
                            }
                        } finally { bitmap.recycle() }
                    }
                }
            }
        } catch (_: Exception) {
            // After publishing a pipe, compression failure is signalled by EOF;
            // the client must validate the complete image, not trust ok alone.
            if (!replied) try { send(Message.obtain(null, what).apply { arg2 = token; data = Bundle().apply { putBoolean("ok", false) } }) } catch (_: Exception) { }
        }
    }
}

/** No Binder pixel payload. Both local pipe ends close on send/compress failure.
 * Binder duplicates the reader for the remote client; close our reader before
 * writing so a cancelled client causes a broken pipe rather than a stuck writer. */
internal fun streamPdfPixels(bitmap: Bitmap, pipe: Array<ParcelFileDescriptor> = ParcelFileDescriptor.createPipe(), send: (ParcelFileDescriptor) -> Unit) {
    try {
        try { send(pipe[0]) } finally { pipe[0].close() }
        ParcelFileDescriptor.AutoCloseOutputStream(pipe[1]).use {
            if (!bitmap.compress(Bitmap.CompressFormat.PNG, 100, it)) throw IOException("PDF image encoding failed")
        }
    } finally {
        for (end in pipe) try { end.close() } catch (_: IOException) { }
    }
}
