plugins {
    id("com.android.application")
}

android {
    namespace = "dev.rimelight.space"
    compileSdk = 37
    ndkVersion = "28.2.13676358"

    defaultConfig {
        applicationId = "dev.rimelight.space"
        minSdk = 26
        targetSdk = 37
        versionCode = 1
        versionName = "0.1.0"
        // The Rust cdylib is built by cargo-ndk into src/main/jniLibs/<abi>/.
        ndk {
            abiFilters += listOf("x86_64")
        }
    }

    buildTypes {
        debug {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
