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

    // An impersonator built from this very source, differing only in package id.
    //
    // Presence is currently decided by a service UUID and a fixed payload, both
    // published in this repository. That means the cheapest possible attack is
    // not writing an app at all -- it is installing a second copy of ours. This
    // flavour makes that attack executable, so "anyone can impersonate the
    // phone" stops being an argument and becomes a test.
    //
    // It must stay buildable. The day the Mac can tell these two apart is the
    // day the impersonation test flips from unlocking to demanding a password,
    // and that flip is the only proof the fix works.
    flavorDimensions += "identity"
    productFlavors {
        create("genuine") {
            dimension = "identity"
        }
        create("imposter") {
            dimension = "identity"
            applicationId = "ai.repose.imposter"
            resValue("string", "app_name", "BLE Imposter")
        }
    }
}
