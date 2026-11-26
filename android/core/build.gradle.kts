plugins {
    alias(libs.plugins.kotlin.jvm)
}

group = "com.trulinc.board"

java {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
    }
}

dependencies {
    // The app brings JNA's Android build (the aar, with its native part);
    // the JVM tests use the jar.
    compileOnly(libs.jna)
    testImplementation(libs.jna)
    testImplementation(libs.junit4)
}

// The tests call the host build of board-ffi: `cargo build -p board-ffi` first.
val hostLibrary = layout.projectDirectory.dir("../../target/debug")

tasks.test {
    inputs.files(fileTree(hostLibrary) { include("libboard_ffi.*") }).withPropertyName("boardFfi")
    systemProperty("jna.library.path", hostLibrary.asFile.absolutePath)
}
