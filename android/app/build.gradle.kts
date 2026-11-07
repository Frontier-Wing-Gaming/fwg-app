import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

// The UI and all app logic are the Rust library `fwg_app` (crates/fwg-app). Gradle builds it with
// cargo-ndk before packaging, so `gradlew assembleDebug` / `bundleRelease` is all that's needed.
val rustRoot = rootProject.projectDir.parentFile
val rustAbis = (findProperty("fwg.abis") as String? ?: "arm64-v8a").split(",").map { it.trim() }
val rustOut = layout.buildDirectory.dir("rustJniLibs")
val firebaseRes = layout.buildDirectory.dir("generated/firebaseRes")

// versionName comes from the workspace Cargo.toml so the app and the Rust crates share one version.
val appVersion: String = Regex("""(?m)^\[workspace\.package\][^\[]*?^version\s*=\s*"([^"]+)"""")
    .find(rustRoot.resolve("Cargo.toml").readText())?.groupValues?.get(1)
    ?: throw GradleException("version not found in [workspace.package] of Cargo.toml")

// Release signing: android/keystore.properties (not in git), see README "Release builds".
//   storeFile=upload-keystore.jks   (relative to android/)
//   storePassword=…  keyAlias=…  keyPassword=…
val keystoreProps = Properties().apply {
    val f = rootProject.file("keystore.properties")
    if (f.exists()) f.inputStream().use { load(it) }
}
val hasReleaseKey = keystoreProps.getProperty("storeFile") != null

android {
    namespace = "network.frontierwinggaming.app"
    compileSdk = 36
    ndkVersion = "28.2.13676358"

    defaultConfig {
        applicationId = "network.frontierwinggaming.app"
        minSdk = 26
        targetSdk = 36
        versionCode = (findProperty("fwg.versionCode") as String?)?.toInt() ?: 1
        versionName = appVersion
        ndk { abiFilters += rustAbis }
    }

    signingConfigs {
        if (hasReleaseKey) {
            create("release") {
                storeFile = rootProject.file(keystoreProps.getProperty("storeFile"))
                storePassword = keystoreProps.getProperty("storePassword")
                keyAlias = keystoreProps.getProperty("keyAlias")
                keyPassword = keystoreProps.getProperty("keyPassword")
            }
        }
    }

    buildTypes {
        // Debug installs next to the Play Store app (different package and name), so testing never
        // needs the store version uninstalled.
        debug {
            applicationIdSuffix = ".dev"
            manifestPlaceholders["appLabel"] = "FWG Dev"
        }
        release {
            isMinifyEnabled = false
            manifestPlaceholders["appLabel"] = "@string/app_name"
            if (hasReleaseKey) signingConfig = signingConfigs.getByName("release")
        }
    }

    sourceSets["main"].jniLibs.srcDir(rustOut)
    sourceSets["main"].res.srcDir(firebaseRes)

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    packaging {
        jniLibs.useLegacyPackaging = false
    }
}

kotlin {
    jvmToolchain(17)
}

dependencies {
    implementation(platform("com.google.firebase:firebase-bom:35.1.0"))
    implementation("com.google.firebase:firebase-messaging")
}

// Firebase reads its settings from string resources (google_app_id, …). The google-services Gradle
// plugin would generate them too, but it insists on a Firebase client for every package, and debug
// builds use "<package>.dev". This writes the Play package's values for every build type instead.
// app/google-services.json is not in git; without it the app builds and push is reported as
// unavailable.
val firebaseConfig by tasks.registering {
    val json = file("google-services.json")
    val out = firebaseRes.get().file("values/firebase.xml").asFile
    inputs.files(fileTree(projectDir) { include("google-services.json") })
    outputs.file(out)
    doLast {
        out.parentFile.mkdirs()
        if (!json.exists()) {
            out.writeText("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<resources />\n")
            logger.warn("android/app/google-services.json missing: push notifications disabled in this build")
            return@doLast
        }
        @Suppress("UNCHECKED_CAST")
        val root = groovy.json.JsonSlurper().parse(json) as Map<String, Any?>
        val project = root["project_info"] as Map<String, Any?>
        val client = (root["client"] as List<Map<String, Any?>>).first {
            val info = (it["client_info"] as Map<String, Any?>)["android_client_info"] as Map<String, Any?>
            info["package_name"] == "network.frontierwinggaming.app"
        }
        val appId = (client["client_info"] as Map<String, Any?>)["mobilesdk_app_id"]
        val apiKey = ((client["api_key"] as List<Map<String, Any?>>).first())["current_key"]
        val values = mapOf(
            "google_app_id" to appId,
            "google_api_key" to apiKey,
            "gcm_defaultSenderId" to project["project_number"],
            "project_id" to project["project_id"],
            "google_storage_bucket" to project["storage_bucket"],
        ).filterValues { it != null }
        out.writeText(buildString {
            append("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<resources>\n")
            values.forEach { (k, v) -> append("    <string name=\"$k\" translatable=\"false\">$v</string>\n") }
            append("</resources>\n")
        })
    }
}

val cargoBuild by tasks.registering(Exec::class) {
    description = "Builds the Rust UI library for Android with cargo-ndk."
    workingDir = rustRoot
    val sdk = android.sdkDirectory
    val ndk = android.ndkDirectory
    environment("ANDROID_HOME", sdk.absolutePath)
    environment("ANDROID_NDK_HOME", ndk.absolutePath)
    // Slint's backend compiles a Java helper against this jar (cargo-ndk's -P would pick API 26).
    environment("ANDROID_JAR", sdk.resolve("platforms/android-${android.compileSdk}/android.jar").absolutePath)
    val args = mutableListOf("ndk", "-P", "26", "-o", rustOut.get().asFile.absolutePath)
    rustAbis.forEach { args += listOf("-t", it) }
    args += listOf("build", "-p", "fwg-app", "--lib", "--release")
    commandLine(listOf("cargo") + args)
    outputs.dir(rustOut)
    outputs.upToDateWhen { false } // cargo does its own up-to-date check
}

tasks.named("preBuild") { dependsOn(cargoBuild, firebaseConfig) }

// Play rejects unsigned bundles and versionCodes it has already seen; fail early with a clear
// message instead of after the Rust build.
gradle.taskGraph.whenReady {
    if (allTasks.any { it.name == "bundleRelease" || it.name == "assembleRelease" }) {
        if (!hasReleaseKey) {
            throw GradleException("Release builds need android/keystore.properties with the Play upload key (see README).")
        }
        if (findProperty("fwg.versionCode") == null) {
            throw GradleException("Pass the next Play versionCode: build.ps1 -Release -VersionCode <n> (see README).")
        }
    }
}

