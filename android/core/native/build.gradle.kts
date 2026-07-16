plugins {
    alias(libs.plugins.android.library)
}

android {
    namespace = "ai.mukei.android.core.nativebridge"
    compileSdk = 37

    defaultConfig {
        minSdk = 26
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    implementation(project(":core:protocol"))
}
