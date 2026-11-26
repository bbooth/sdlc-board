plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.compose)
}

// CI passes the run number so each build installs over the last one.
val appVersionCode = providers.gradleProperty("board.versionCode").getOrElse("1").toInt()

android {
    namespace = "com.trulinc.board"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.trulinc.board"
        minSdk = 26
        targetSdk = 35
        versionCode = appVersionCode
        versionName = "0.1.$appVersionCode"
        // board-ffi is built for arm64 phones only (README "Build").
        ndk { abiFilters += "arm64-v8a" }
    }

    buildFeatures {
        compose = true
    }

    buildTypes {
        release {
            // R8 would strip the JNA classes the bindings reach by reflection.
            isMinifyEnabled = false
            // Left unsigned: CI signs the APK with the release key (README "Release key").
        }
    }

    testOptions {
        unitTests {
            isIncludeAndroidResources = true
            // The tests call the host build of board-ffi: `cargo build -p board-ffi` first.
            all {
                it.systemProperty("jna.library.path", rootProject.file("../target/debug").absolutePath)
                // CI has only the log: show why a test failed, not just where.
                it.testLogging.exceptionFormat = org.gradle.api.tasks.testing.logging.TestExceptionFormat.FULL
            }
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    implementation("com.trulinc.board:core")
    // JNA's Android build, with its native part for each ABI.
    implementation("net.java.dev.jna:jna:${libs.versions.jna.get()}@aar")

    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.compose.material3)
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.kotlinx.coroutines.android)
    debugImplementation(libs.androidx.compose.ui.test.manifest)

    testImplementation(libs.junit4)
    testImplementation(libs.robolectric)
    testImplementation(libs.androidx.test.core)
    testImplementation(libs.androidx.compose.ui.test.junit4)
    testImplementation(libs.okhttp.mockwebserver)
    // The JVM build of JNA, with its native part for the host.
    testImplementation(libs.jna)
}
