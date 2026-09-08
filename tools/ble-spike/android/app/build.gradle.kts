plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "ai.repose.blespike"
    compileSdk = 36

    defaultConfig {
        applicationId = "ai.repose.blespike"
        minSdk = 31
        targetSdk = 36
        versionCode = 1
        versionName = "0.1"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlin {
        compilerOptions {
            jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
        }
    }

    sourceSets {
        getByName("main").kotlin.srcDirs("src/main/kotlin")
    }

    buildTypes {
        getByName("debug") {
            isMinifyEnabled = false
        }
    }
}
