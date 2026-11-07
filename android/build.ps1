<#
Builds the Android app (Rust UI library + Kotlin shell) and optionally installs it.

  .\android\build.ps1                 # debug APK  -> android\app\build\outputs\apk\debug
  .\android\build.ps1 -Install        # ... and install on the connected phone/emulator
  .\android\build.ps1 -Release -VersionCode 12   # signed release AAB for Play (see README)
  .\android\build.ps1 -Abis arm64-v8a,x86_64   # also build for the x86_64 emulator

Tool locations come from the environment (ANDROID_HOME, ANDROID_NDK, CARGO_HOME, RUSTUP_HOME,
GRADLE_USER_HOME, LIBCLANG_PATH, SKIA_TOOLS). Machine-specific defaults can go in android\local.ps1
(ignored by git), which may call `Default NAME value` and set `$gitSchannel = $true`.
#>
param([switch]$Install, [switch]$Release, [string]$Abis = '', [int]$VersionCode = 0)
$ErrorActionPreference = 'Stop'

function Default($name, $value) { if (-not [Environment]::GetEnvironmentVariable($name)) { Set-Item "env:$name" $value } }
$gitSchannel = $false
$local = Join-Path $PSScriptRoot 'local.ps1'
if (Test-Path $local) { . $local }
if (-not $env:ANDROID_HOME) { throw 'Set ANDROID_HOME to the Android SDK (or add it to android\local.ps1).' }
if ($env:CARGO_HOME) { $env:Path = "$env:CARGO_HOME\bin;$env:Path" }
$env:Path = "$env:ANDROID_HOME\platform-tools;$env:Path"

# 32-bit ARM: rust-skia publishes no prebuilt armv7 Skia, so skia-bindings builds it from source
# (once; the result stays in target\). That needs ninja, a `python3` that isn't the Store stub,
# libclang >= the NDK's clang (19) for bindgen, and git long paths for Skia's dependencies.
# See README "32-bit ARM". SKIA_TOOLS\bin holds ninja and a python3.cmd shim.
if ($env:SKIA_TOOLS) { $env:Path = "$env:SKIA_TOOLS\bin;$env:Path" }
if (-not $env:ANDROID_NDK) { $env:ANDROID_NDK = "$env:ANDROID_HOME\ndk\28.2.13676358" }
# A bare libclang.dll can't find clang's built-in headers (stddef.h …), so bindgen needs
# -resource-dir. cargo-ndk overwrites BINDGEN_EXTRA_CLANG_ARGS_armv7_linux_androideabi, but bindgen
# checks the hyphenated name first, so this one wins; it repeats cargo-ndk's sysroot arguments.
$tc = "$env:ANDROID_NDK\toolchains\llvm\prebuilt\windows-x86_64".Replace('\', '/')
[Environment]::SetEnvironmentVariable('BINDGEN_EXTRA_CLANG_ARGS_armv7-linux-androideabi',
    "--sysroot=$tc/sysroot -I$tc/sysroot/usr/include/arm-linux-androideabi -resource-dir=$tc/lib/clang/19", 'Process')
$env:GIT_CONFIG_COUNT = if ($gitSchannel) { '2' } else { '1' }
$env:GIT_CONFIG_KEY_0 = 'core.longpaths'; $env:GIT_CONFIG_VALUE_0 = 'true'
$env:GIT_CONFIG_KEY_1 = 'http.sslBackend'; $env:GIT_CONFIG_VALUE_1 = 'schannel'

# Release bundles carry every ABI Play serves; debug builds only the phone's.
if (-not $Abis) { $Abis = if ($Release) { 'arm64-v8a,armeabi-v7a,x86_64' } else { 'arm64-v8a' } }

Push-Location $PSScriptRoot
try {
    $task = if ($Release) { 'bundleRelease' } else { 'assembleDebug' }
    $props = @("-Pfwg.abis=$Abis")
    if ($VersionCode) { $props += "-Pfwg.versionCode=$VersionCode" }
    .\gradlew.bat $task @props --console=plain
    if ($LASTEXITCODE) { throw "Gradle failed ($LASTEXITCODE)" }
    if ($Install -and -not $Release) {
        adb install -r app\build\outputs\apk\debug\app-debug.apk
        if ($LASTEXITCODE) { throw 'adb install failed' }
        # Debug builds use the ".dev" package so they sit next to the Play Store app.
        adb shell am start -n network.frontierwinggaming.app.dev/network.frontierwinggaming.app.MainActivity
    }
    if ($Release) { Write-Host "Bundle: $PSScriptRoot\app\build\outputs\bundle\release\app-release.aab" }
} finally { Pop-Location }
