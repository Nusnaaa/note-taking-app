package org.stemnotebook.backend

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.IBinder
import android.os.Message
import android.os.Messenger
import java.io.Closeable
import java.util.UUID
import java.util.concurrent.Executor

/** Transport only; source policy and notebook paths never cross this interface. */
internal interface PdfConnection : Closeable {
    fun connect(ready: (Messenger) -> Unit, failed: () -> Unit): Boolean
}
internal class AndroidPdfConnection(private val context: Context, private val callbacks: Executor) : PdfConnection {
    private val lock = Any()
    private var closed = false
    private var registered = false
    private var remote: Messenger? = null
    private var binder: IBinder? = null
    private var onDeath: (() -> Unit)? = null
    private val death = IBinder.DeathRecipient { synchronized(lock) { onDeath }?.invoke() }
    private val connection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName, service: IBinder) {
            val receiver: ((Messenger) -> Unit)?
            val messenger = Messenger(service)
            synchronized(lock) {
                if (closed) return
                remote = messenger; binder = service
                try { service.linkToDeath(death, 0) } catch (_: Exception) { onDeath?.invoke(); return }
                receiver = onReady
            }
            receiver?.invoke(messenger)
        }
        override fun onServiceDisconnected(name: ComponentName) { synchronized(lock) { if (closed) null else onDeath }?.invoke() }
        override fun onBindingDied(name: ComponentName) = onServiceDisconnected(name)
        override fun onNullBinding(name: ComponentName) = onServiceDisconnected(name)
    }
    private var onReady: ((Messenger) -> Unit)? = null
    override fun connect(ready: (Messenger) -> Unit, failed: () -> Unit): Boolean {
        synchronized(lock) {
            if (closed) return false
            onReady = ready; onDeath = failed
        }
        val accepted = context.bindIsolatedService(
            Intent(context, PdfWorkerService::class.java),
            Context.BIND_AUTO_CREATE or Context.BIND_NOT_FOREGROUND,
            "pdf-${UUID.randomUUID()}", callbacks, connection,
        )
        val lateClose = synchronized(lock) { registered = accepted; closed && accepted }
        if (lateClose) unbind()
        return accepted
    }
    private fun unbind() {
        val remove = synchronized(lock) { val value = registered; registered = false; value }
        if (remove) try { context.unbindService(connection) } catch (_: IllegalArgumentException) { }
    }
    override fun close() {
        val resources = synchronized(lock) {
            if (closed) return
            closed = true
            val result = remote to binder
            remote = null; binder = null; onReady = null; onDeath = null
            result
        }
        try {
            try { resources.first?.send(Message.obtain(null, 99)) } catch (_: Exception) { }
            try { resources.second?.unlinkToDeath(death, 0) } catch (_: Exception) { }
        } finally { unbind() }
    }
}
