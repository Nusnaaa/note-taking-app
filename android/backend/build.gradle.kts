plugins { id("com.android.library"); id("org.jetbrains.kotlin.android") }
dependencyLocking { lockAllConfigurations() }
android {
    namespace = "org.stemnotebook.backend"
    compileSdk = 35
    defaultConfig { minSdk = 30; testInstrumentationRunner = "android.test.InstrumentationTestRunner" }
    compileOptions { sourceCompatibility = JavaVersion.VERSION_17; targetCompatibility = JavaVersion.VERSION_17 }
    kotlinOptions { jvmTarget = "17" }
    sourceSets["main"].jniLibs.srcDir("../../target/android-jni")
    testOptions { unitTests.isReturnDefaultValues = false; unitTests.isIncludeAndroidResources = true }
    testOptions.unitTests.all {
        for (property in listOf("https.proxyHost", "https.proxyPort", "http.proxyHost", "http.proxyPort", "javax.net.ssl.trustStore")) {
            System.getProperty(property)?.let { value -> it.systemProperty(property, value) }
        }
        it.systemProperty("robolectric.dependency.repo.url", "https://repo.maven.apache.org/maven2")
    }
    lint { abortOnError = true }
}
dependencies { testImplementation("junit:junit:4.13.2"); testImplementation("org.robolectric:robolectric:4.14.1") }
