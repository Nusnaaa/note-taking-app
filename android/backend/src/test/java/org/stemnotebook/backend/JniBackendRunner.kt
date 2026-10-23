package org.stemnotebook.backend

import org.junit.runners.model.FrameworkMethod
import org.robolectric.RobolectricTestRunner
import org.robolectric.internal.bytecode.InstrumentationConfiguration

/** Keep the pure Java JNI entrypoint in the parent loader. Robolectric must not
 * rewrite native methods into shadows or load the same .so in each sandbox. */
class JniBackendRunner(testClass: Class<*>) : RobolectricTestRunner(testClass) {
    override fun createClassLoaderConfig(method: FrameworkMethod): InstrumentationConfiguration =
        InstrumentationConfiguration.Builder(super.createClassLoaderConfig(method))
            .doNotAcquireClass("org.stemnotebook.backend.NativeBridge")
            .doNotAcquireClass("org.stemnotebook.backend.NativeBridge\$Verification")
            .doNotAcquireClass("org.stemnotebook.backend.NativeBridge\$PdfGrant")
            .build()
}
