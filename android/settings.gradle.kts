pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "sdlc-board-android"

// core is a build of its own: the UniFFI bindings and their JVM tests build
// without the Android SDK (`./gradlew :core:test`).
includeBuild("core")
include(":app")
