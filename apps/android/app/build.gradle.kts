import java.io.ByteArrayOutputStream

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
    id("org.jetbrains.kotlin.plugin.serialization")
}

android {
    namespace = "io.perch.android"
    compileSdk = 35

    defaultConfig {
        applicationId = "io.perch.android"
        minSdk = 29            // MediaProjection + Presentation + USB host stable from here
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
        ndk { abiFilters += listOf("arm64-v8a") }   // S24 Ultra; add x86_64 for emulator
        externalNativeBuild { cmake { arguments += "-DANDROID_STL=c++_shared" } }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }
    buildFeatures {
        compose = true
        aidl = true
        buildConfig = true
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    externalNativeBuild { cmake { path = file("src/main/cpp/CMakeLists.txt") } }
    sourceSets["main"].jniLibs.srcDirs("src/main/jniLibs")
}

// ---------------------------------------------------------------------------
// Rust: build perch-ffi for Android with cargo-ndk and drop the .so into jniLibs.
//   cargo install cargo-ndk
//   rustup target add aarch64-linux-android x86_64-linux-android
// Run `./gradlew cargoBuild` (or it runs automatically before preBuild).
// ---------------------------------------------------------------------------
val cargoBuild by tasks.registering(Exec::class) {
    group = "rust"
    description = "Build perch-ffi for Android via cargo-ndk"
    workingDir = rootProject.file("../..")   // repo root
    val abis = listOf("arm64-v8a")
    val args = mutableListOf("ndk")
    abis.forEach { args += listOf("-t", it) }
    args += listOf("-o", project.file("src/main/jniLibs").absolutePath, "build", "--release", "-p", "perch-ffi", "--features", "rokid")
    commandLine(listOf("cargo") + args)
    isIgnoreExitValue = false
}
tasks.named("preBuild") { dependsOn(cargoBuild) }

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.12.01")
    implementation(composeBom)
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.7")
    implementation("androidx.lifecycle:lifecycle-service:2.8.7")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.datastore:datastore-preferences:1.1.1")
    implementation("com.squareup.okhttp3:okhttp:4.12.0")
    // Privileged helper (trusted virtual displays, input injection) via Shizuku.
    implementation("dev.rikka.shizuku:api:13.1.5")
    implementation("dev.rikka.shizuku:provider:13.1.5")
    implementation("org.lsposed.hiddenapibypass:hiddenapibypass:4.3")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.7.3")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")
    debugImplementation("androidx.compose.ui:ui-tooling")
    testImplementation("junit:junit:4.13.2")
}
