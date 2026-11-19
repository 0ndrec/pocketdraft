plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

// The ABI to build the Rust library for and package: `-Pabi=x86_64` for the x86_64 emulator.
val abi = (findProperty("abi") as String?) ?: "arm64-v8a"
val triples = mapOf(
    "arm64-v8a" to "aarch64-linux-android",
    "armeabi-v7a" to "armv7-linux-androideabi",
    "x86_64" to "x86_64-linux-android",
    "x86" to "i686-linux-android",
)
val triple = triples[abi] ?: error("Unknown ABI $abi: one of ${triples.keys}")
val minApi = 26

android {
    namespace = "app.pocketdraft"
    compileSdk = 36
    // The NDK cargo-ndk builds the Rust library with; Gradle strips the library with it.
    ndkVersion = "27.1.12297006"

    defaultConfig {
        // The id is set in gradle.properties (`appId`).
        applicationId = property("appId") as String
        minSdk = minApi
        targetSdk = 36
        versionCode = 5
        versionName = "0.3.0-alpha.3"
        ndk { abiFilters += abi }
    }

    // A release key from the environment (the CI build, from the repository's secrets):
    // POCKETDRAFT_KEYSTORE (a file), its password, the key's alias and password.
    val keystore = System.getenv("POCKETDRAFT_KEYSTORE")?.takeIf { it.isNotBlank() && file(it).exists() }
    if (keystore != null) {
        signingConfigs.create("release") {
            storeFile = file(keystore)
            storePassword = System.getenv("POCKETDRAFT_KEYSTORE_PASSWORD")
            keyAlias = System.getenv("POCKETDRAFT_KEY_ALIAS")
            keyPassword = System.getenv("POCKETDRAFT_KEY_PASSWORD")
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            // Unsigned by default, as F-Droid and a release key expect. `-PdebugSigned` signs
            // with the debug key, for trying a release build on a device; a key in the
            // environment (above) signs it in CI.
            when {
                project.hasProperty("debugSigned") -> signingConfig = signingConfigs.getByName("debug")
                keystore != null -> signingConfig = signingConfigs.getByName("release")
            }
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    buildFeatures { compose = true }
    packaging {
        // cargo-ndk also copies a dynamic build of a dependency that nothing loads.
        jniLibs { excludes += "**/libdraftwright_wasm*.so" }
    }
    // No dependency list in the APK's signing block: only Google can read it.
    dependenciesInfo {
        includeInApk = false
        includeInBundle = false
    }
}

// --- the Rust core ----------------------------------------------------------------------
// Built here so that one Gradle command makes the whole app on any OS: cargo-ndk builds the
// library, UniFFI generates its Kotlin bindings from it, and both are build outputs (never
// sources in the repository). Needs `cargo` on the PATH, the Rust target of the ABI, and
// `cargo install cargo-ndk`.
val rustDir = rootProject.file("../rust")
val jniLibs = layout.buildDirectory.dir("rust/jniLibs")
val bindings = layout.buildDirectory.dir("rust/bindings")
val rustSources = fileTree(rustDir) {
    include("src/**", "assets/**", "Cargo.toml", "Cargo.lock", "uniffi-bindgen/src/**", "uniffi-bindgen/Cargo.*")
}

val cargoBuild = tasks.register<Exec>("cargoBuild") {
    description = "Builds the Rust library for $abi with cargo-ndk."
    workingDir = rustDir
    inputs.files(rustSources)
    inputs.property("abi", abi)
    outputs.dir(jniLibs)
    // rustc overflows its default stack on one dependency (on Windows at least).
    environment("RUST_MIN_STACK", "268435456")
    environment("ANDROID_NDK_HOME", System.getenv("ANDROID_NDK_HOME") ?: android.ndkDirectory.absolutePath)
    commandLine(
        "cargo", "ndk", "-t", abi, "-P", minApi.toString(),
        "-o", jniLibs.get().asFile.absolutePath,
        "build", "--release", "--lib", "--locked",
    )
}

val uniffiBindings = tasks.register<Exec>("uniffiBindings") {
    description = "Generates the Kotlin bindings from the built Rust library."
    dependsOn(cargoBuild)
    // From the crate: the generator reads its Cargo metadata. The library in `target` keeps
    // its symbols, which hold the interface; Gradle strips the copy it packages.
    workingDir = rustDir
    inputs.files(rustSources)
    inputs.property("abi", abi)
    outputs.dir(bindings)
    environment("RUST_MIN_STACK", "268435456")
    commandLine(
        "cargo", "run", "--release", "--locked", "--manifest-path", "uniffi-bindgen/Cargo.toml", "--",
        "generate", "--library", File(rustDir, "target/$triple/release/libdraftwright_ffi.so").absolutePath,
        "--language", "kotlin", "--out-dir", bindings.get().asFile.absolutePath, "--no-format",
    )
}

android.sourceSets.getByName("main") {
    jniLibs.srcDir(this@Build_gradle.jniLibs)
    java.srcDir(bindings)
}
tasks.named("preBuild") { dependsOn(uniffiBindings) }

dependencies {
    val bom = platform("androidx.compose:compose-bom:2024.12.01")
    implementation(bom)
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")
    // UniFFI's Kotlin bindings call the Rust library through JNA.
    implementation("net.java.dev.jna:jna:5.15.0@aar")
    // Library thumbnails are small SVGs.
    implementation("com.caverock:androidsvg-aar:1.4")
}
