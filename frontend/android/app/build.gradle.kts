plugins {
    id("com.android.application")
    id("kotlin-android")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
}

val supportedAndroidAbis = listOf("arm64-v8a")

android {
    namespace = "com.continuum.app"
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
        applicationId = "com.continuum.app"
        minSdk = maxOf(flutter.minSdkVersion, 23)
        targetSdk = flutter.targetSdkVersion
        versionCode = flutter.versionCode
        versionName = flutter.versionName

        ndk {
            abiFilters.clear()
            abiFilters.addAll(supportedAndroidAbis)
        }

        externalNativeBuild {
            cmake {
                cppFlags += "-std=c++17"
            }
        }
    }

    buildTypes {
        debug {
            ndk {
                abiFilters.clear()
                abiFilters.addAll(supportedAndroidAbis)
            }
        }

        release {
            signingConfig = signingConfigs.getByName("debug")
            ndk {
                abiFilters.clear()
                abiFilters.addAll(supportedAndroidAbis)
            }
        }
    }

    externalNativeBuild {
        cmake {
            path = file("src/main/cpp/CMakeLists.txt")
        }
    }

    sourceSets {
        getByName("main") {
            jniLibs.setSrcDirs(emptyList<String>())
        }
    }
}

flutter {
    source = "../.."
}

val rustAndroidTarget = "aarch64-linux-android"
val rustAndroidAbi = "arm64-v8a"
val rustAndroidApi = maxOf(flutter.minSdkVersion, 23)
val repoRoot = rootProject.projectDir.parentFile.parentFile
val rustOutput = file("$repoRoot/target/$rustAndroidTarget/release/libcontinuum_core.a")
val androidRustOutput = file("src/main/rustLibs/$rustAndroidAbi/libcontinuum_core.a")
val hostTag =
    when {
        System.getProperty("os.name").contains("Windows", ignoreCase = true) -> "windows-x86_64"
        System.getProperty("os.name").contains("Mac", ignoreCase = true) -> "darwin-x86_64"
        else -> "linux-x86_64"
    }
val linkerSuffix = if (hostTag.startsWith("windows")) ".cmd" else ""

afterEvaluate {
    android.buildTypes.configureEach {
        ndk.abiFilters.clear()
        ndk.abiFilters.addAll(supportedAndroidAbis)
    }
}

val buildRustAndroid by tasks.registering(Exec::class) {
    val ndkRoot = android.ndkDirectory
    val llvmBin = file("$ndkRoot/toolchains/llvm/prebuilt/$hostTag/bin")
    val linker = file("$llvmBin/aarch64-linux-android$rustAndroidApi-clang$linkerSuffix")

    inputs.dir(file("$repoRoot/backend/src"))
    inputs.file(file("$repoRoot/backend/Cargo.toml"))
    inputs.file(file("$repoRoot/Cargo.toml"))
    outputs.file(androidRustOutput)

    doFirst {
        require(linker.exists()) {
            "Android NDK linker not found: ${linker.absolutePath}"
        }
        androidRustOutput.parentFile.mkdirs()
    }

    workingDir = repoRoot
    environment("CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER", linker.absolutePath)
    commandLine(
        "cargo",
        "build",
        "-p",
        "continuum",
        "--lib",
        "--release",
        "--no-default-features",
        "--target",
        rustAndroidTarget,
    )

    doLast {
        copy {
            from(rustOutput)
            into(androidRustOutput.parentFile)
        }
    }
}

tasks.matching {
    it.name.contains("CMake") ||
        it.name.contains("JniLib") ||
        it.name.startsWith("pre")
}.configureEach {
    dependsOn(buildRustAndroid)
}

