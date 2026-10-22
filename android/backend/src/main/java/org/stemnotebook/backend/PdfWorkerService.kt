package org.stemnotebook.backend

import android.app.Service
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.pdf.PdfRenderer
import android.os.*

/** OS-isolated parser process has no notebook path, credentials, network permission
 * or provider grants. Only the explicit read-only descriptor for this request. */
class PdfWorkerService : Service() {
    private lateinit var thread: HandlerThread
    override fun onCreate() { super.onCreate(); thread = HandlerThread("pdf-render").apply { start() } }
    private val busy = java.util.concurrent.atomic.AtomicBoolean(false)
    override fun onBind(intent: Intent): IBinder = Messenger(Handler(Looper.getMainLooper()) { message ->
        // The control thread stays responsive while the native parser is occupied.
        if (message.what == 99) { Process.killProcess(Process.myPid()); return@Handler true }
        if (!busy.compareAndSet(false, true)) {
            val reply = Message.obtain(null, message.what)
            reply.data = Bundle().apply { putBoolean("ok", false) }
            try { message.replyTo?.send(reply) } catch (_: Exception) { }
            return@Handler true
        }
        val copy = Message.obtain(message)
        Handler(thread.looper).post { try { render(copy) } finally { busy.set(false); copy.recycle() } }
        true
    }).binder
    @Suppress("DEPRECATION")
    private fun render(message: Message) {
        val reply = Message.obtain(null, message.what)
        try {
            val descriptor = message.data.getParcelable<ParcelFileDescriptor>("descriptor") ?: error("descriptor missing")
            val page = message.arg1
            val width = message.data.getInt("width")
            val height = message.data.getInt("height")
            require(width in 1..4096 && height in 1..4096 && width.toLong() * height <= 8_000_000)
            descriptor.use { fd -> PdfRenderer(fd).use { renderer ->
                require(page in 0 until renderer.pageCount)
                renderer.openPage(page).use { source ->
                    val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
                    try {
                        bitmap.eraseColor(android.graphics.Color.WHITE)
                        source.render(bitmap, null, null, PdfRenderer.Page.RENDER_MODE_FOR_DISPLAY)
                        // Never pass pixels in a Binder transaction (1 MiB boundary).
                        val pipe = ParcelFileDescriptor.createPipe()
                        reply.data = Bundle().apply { putParcelable("pixels", pipe[0]); putInt("pages", renderer.pageCount); putBoolean("ok", true) }
                        message.replyTo.send(reply)
                        pipe[0].close()
                        ParcelFileDescriptor.AutoCloseOutputStream(pipe[1]).use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
                    } finally { bitmap.recycle() }
                }
            } }
        } catch (_: Exception) {
            reply.data = Bundle().apply { putBoolean("ok", false) }
            try { message.replyTo?.send(reply) } catch (_: Exception) { }
        }
    }
    override fun onDestroy() { thread.quitSafely(); super.onDestroy() }
}
