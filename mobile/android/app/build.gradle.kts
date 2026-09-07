plugins {
    id("com.android.application")
    id("kotlin-android")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
}

val releaseGateError =
    "Repose Unlock release is blocked until Task 10 production signing is configured."
val mobileAppProjectPath = project.path

gradle.taskGraph.whenReady {
    val releaseTaskRequested = allTasks.any { task ->
        task.project.path == mobileAppProjectPath && task.name.contains("Release")
    }
    if (releaseTaskRequested) {
        throw GradleException(releaseGateError)
    }
}

android {
    namespace = "ai.repose.repose_unlock"
    compileSdk = flutter.compileSdkVersion
    ndkVersion = flutter.ndkVersion

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = JavaVersion.VERSION_17.toString()
    }

    defaultConfig {
        // TODO: Specify your own unique Application ID (https://developer.android.com/studio/build/application-id.html).
        applicationId = "ai.repose.repose_unlock"
        // You can update the following values to match your application needs.
        // For more information, see: https://flutter.dev/to/review-gradle-config.
        minSdk = flutter.minSdkVersion
        targetSdk = flutter.targetSdkVersion
        versionCode = flutter.versionCode
        versionName = flutter.versionName
    }

}

flutter {
    source = "../.."
}
