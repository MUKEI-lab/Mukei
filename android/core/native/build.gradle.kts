plugins {
    alias(libs.plugins.android.library)
}

android {
    namespace = "ai.mukei.android.core.nativebridge"
    compileSdk = 37
    ndkVersion = "27.2.12479018"

    defaultConfig {
        minSdk = 26
        consumerProguardFiles("consumer-rules.pro")
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    packaging {
        jniLibs.useLegacyPackaging = false
    }
}

dependencies {
    implementation(project(":core:protocol"))
    implementation("com.google.mlkit:text-recognition-devanagari:16.0.1")
}
