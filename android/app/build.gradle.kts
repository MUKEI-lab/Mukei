import java.util.zip.ZipFile

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace = "ai.mukei.android"
    compileSdk = 37
    ndkVersion = "27.2.12479018"

    defaultConfig {
        applicationId = "ai.mukei.android"
        minSdk = 26
        targetSdk = 37
        versionCode = 70500
        versionName = "0.7.5"
    }

    buildTypes {
        debug {
            isMinifyEnabled = false
            isShrinkResources = false
        }
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
        }
        create("offline") {
            initWith(getByName("release"))
            matchingFallbacks += listOf("release")
            applicationIdSuffix = ".offline"
            versionNameSuffix = "-offline"
        }
    }

    splits {
        abi {
            isEnable = true
            reset()
            include("arm64-v8a", "x86_64")
            isUniversalApk = false
        }
    }

    buildFeatures {
        compose = true
        buildConfig = false
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    packaging {
        jniLibs.useLegacyPackaging = false
        resources.excludes += "/META-INF/{AL2.0,LGPL2.1}"
    }
}

dependencies {
    implementation(project(":core:protocol"))
    implementation(project(":core:native"))
    implementation(project(":core:designsystem"))

    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.ui.tooling.preview)
    implementation(libs.androidx.compose.material3)

    debugImplementation(libs.androidx.compose.ui.tooling)
    testImplementation("junit:junit:4.13.2")
}

// Release APKs must remain self-contained for every supported ABI. This catches
// regressions where the JNI runtime links successfully during CI but Android cannot
// load it on-device because a required packaged shared library was omitted.
tasks.matching { it.name == "assembleRelease" }.configureEach {
    doLast {
        val releaseDir = layout.buildDirectory.dir("outputs/apk/release").get().asFile
        val apks = releaseDir
            .listFiles { file -> file.isFile && file.extension == "apk" }
            ?.sortedBy { it.name }
            .orEmpty()
        check(apks.size == 2) {
            "Expected exactly two ABI-split release APKs, found ${apks.size}: ${apks.map { it.name }}"
        }

        listOf("arm64-v8a", "x86_64").forEach { abi ->
            val apk = apks.singleOrNull { it.name.contains(abi) }
                ?: error("Missing release APK for ABI $abi")
            val requiredEntries = listOf(
                "lib/$abi/libmukei_android.so",
                "lib/$abi/libmukei_llama_native.so",
                "lib/$abi/libc++_shared.so",
            )
            ZipFile(apk).use { zip ->
                requiredEntries.forEach { entry ->
                    check(zip.getEntry(entry) != null) {
                        "${apk.name} is missing required native runtime entry $entry"
                    }
                }
            }
        }
    }
}
