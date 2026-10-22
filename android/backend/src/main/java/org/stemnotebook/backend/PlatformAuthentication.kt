package org.stemnotebook.backend

import android.app.Activity
import android.hardware.biometrics.BiometricManager
import android.hardware.biometrics.BiometricPrompt
import android.os.CancellationSignal
import android.os.SystemClock
import java.util.concurrent.atomic.AtomicBoolean

/** Authentication is local Android/device protection. No independent database
 * encryption is claimed. A proof is usable once, promptly after OS success. */
class PlatformAuthentication(private val activity: Activity) {
    fun request(onSuccess: (NativeBridge.Verification) -> Unit, onFailure: () -> Unit): CancellationSignal {
        val cancellation = CancellationSignal()
        val finished = AtomicBoolean(false)
        BiometricPrompt.Builder(activity)
            .setTitle("Unlock selected notebook content")
            .setAllowedAuthenticators(BiometricManager.Authenticators.BIOMETRIC_STRONG or BiometricManager.Authenticators.DEVICE_CREDENTIAL)
            .build().authenticate(cancellation, activity.mainExecutor, object : BiometricPrompt.AuthenticationCallback() {
                override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                    if (!finished.compareAndSet(false, true)) return
                    val issued = SystemClock.elapsedRealtime()
                    val used = AtomicBoolean(false)
                    onSuccess(NativeBridge.Verification { challenge ->
                        challenge.isNotEmpty() && SystemClock.elapsedRealtime() - issued in 0..30_000 && used.compareAndSet(false, true)
                    })
                }
                override fun onAuthenticationError(errorCode: Int, errString: CharSequence) {
                    if (finished.compareAndSet(false, true)) onFailure()
                }
            })
        return cancellation
    }
}
