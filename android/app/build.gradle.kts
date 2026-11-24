import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

val releaseKeyStorePath = System.getenv("ANDROID_KEYSTORE_PATH")
val releaseKeyStorePassword = System.getenv("ANDROID_KEYSTORE_PASSWORD")
val releaseKeyAlias = System.getenv("ANDROID_KEY_ALIAS")
val releaseKeyPassword = System.getenv("ANDROID_KEY_PASSWORD")
val releaseSigningValues = listOf(
    releaseKeyStorePath,
    releaseKeyStorePassword,
    releaseKeyAlias,
    releaseKeyPassword,
)
val releaseSigningConfigured = releaseSigningValues.any { !it.isNullOrBlank() }
check(!releaseSigningConfigured || releaseSigningValues.all { !it.isNullOrBlank() }) {
    "Set all Android release signing environment variables or leave them all unset."
}

android {
    namespace = "com.scorpionlegend.clicksmith"
    compileSdk = 36

    signingConfigs {
        if (releaseSigningConfigured) {
            create("release") {
                storeFile = file(requireNotNull(releaseKeyStorePath))
                storePassword = requireNotNull(releaseKeyStorePassword)
                keyAlias = requireNotNull(releaseKeyAlias)
                keyPassword = requireNotNull(releaseKeyPassword)
            }
        }
    }

    defaultConfig {
        applicationId = "com.scorpionlegend.clicksmith"
        minSdk = 26
        targetSdk = 36
        versionCode = 2
        versionName = "0.2.0-beta.1"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            if (releaseSigningConfigured) {
                signingConfig = signingConfigs.getByName("release")
            }
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(JvmTarget.JVM_17)
    }
}
