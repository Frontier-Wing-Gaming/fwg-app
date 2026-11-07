# Frontier Gaming Network app

The Frontier Wing Gaming Network app, written in Rust with [Slint](https://slint.dev). It replaces the
earlier Expo / React Native app and uses much less memory. The Android app is on Google Play as
`network.frontierwinggaming.app`; iOS is planned.

## Layout

- `crates/fwg-core`: platform-independent core. Website API client, models, offline cache, staff session,
  news Markdown parser, date formatting, server status text, fallback streamer list and notification routes.
  Storage keys and the cache format match the Expo app, so an update keeps users' data.
- `crates/fwg-app`: the Slint app. `ui/` holds the screens (`pages/`), shared widgets, theme and the data
  types the Rust side fills. `src/app.rs` loads data and handles navigation, `src/view.rs` turns core data
  into screen data, `src/images.rs` downloads, caches and downsizes remote images.
- `android/`: the Gradle project that packages the Rust library as an Android app.

## Desktop

```powershell
cargo run -p fwg-app            # desktop window (debug)
cargo run -p fwg-app --release  # use this one to measure memory
```

Escape goes back. On Windows, settings, the offline cache and the image cache live in
`%LOCALAPPDATA%\FrontierGamingNetwork`.

```powershell
cargo test --workspace                                # unit and mock-server tests
cargo test -p fwg-core --test api -- --ignored live_  # smoke test against the real website API
cargo clippy --workspace --all-targets
```

## Memory

Release builds, after visiting every screen (gallery, staff avatars and a server banner loaded):

| Build | Memory |
|---|---|
| Desktop, software renderer (default) | ~32 MB working set |
| Desktop, OpenGL / femtovg (`SLINT_BACKEND=winit-femtovg`) | ~96 MB working set |
| Android (Skia, GPU) | 147–164 MB PSS |
| Old Expo app on the same phone, same screens | 234–240 MB PSS |

Most of the OpenGL and Android figures is GPU memory (window buffers and Skia). Remote images are decoded
off the UI thread and shrunk to display size (gallery 512 px, avatars 160 px, banners 960 px) before the UI
sees them.

## Android

A Kotlin `MainActivity` (a `NativeActivity` subclass) hosts the Rust library `libfwg_app.so`, which draws
the whole UI with Slint's Android backend (Skia). Gradle runs `cargo ndk` itself, so `gradlew` is all a build
needs.

### Requirements

- Android SDK with platform 36 and NDK 28.2.13676358, JDK 17
- Rust targets `aarch64-linux-android`, `armv7-linux-androideabi`, `x86_64-linux-android`
- `cargo install cargo-ndk`
- For 32-bit ARM only: see [32-bit ARM](#32-bit-arm)

### Building

On Windows, `android/build.ps1` wraps Gradle:

```powershell
.\android\build.ps1                         # debug APK: android\app\build\outputs\apk\debug\app-debug.apk
.\android\build.ps1 -Install                # ... then install and launch it on the connected device
.\android\build.ps1 -Abis arm64-v8a,x86_64  # also build for an x86_64 emulator
.\android\build.ps1 -Release -VersionCode <n>  # signed Play bundle (see below)
```

It reads `ANDROID_HOME`, `ANDROID_NDK`, `CARGO_HOME`, `RUSTUP_HOME`, `GRADLE_USER_HOME`, `LIBCLANG_PATH`
and `SKIA_TOOLS` from the environment. On other systems run Gradle directly from `android/`:

```sh
./gradlew assembleDebug -Pfwg.abis=arm64-v8a
./gradlew bundleRelease -Pfwg.abis=arm64-v8a,armeabi-v7a,x86_64 -Pfwg.versionCode=<n>
```

Debug builds use the package `network.frontierwinggaming.app.dev` ("FWG Dev"), so they install next to the
Play app. They build only `arm64-v8a` unless told otherwise. Release bundles contain arm64-v8a,
armeabi-v7a and x86_64.

### How the pieces fit

- minSdk 26 (Android 8.0), targetSdk 36. The version name comes from `[workspace.package]` in `Cargo.toml`.
- `MainActivity` draws edge to edge; Slint pads the UI with the safe-area insets.
- Links and sharing: Rust calls `MainActivity.openUrl` / `shareText` over JNI (`src/platform.rs`).
- Back: on Android 13+ the activity's `OnBackInvokedCallback` calls `nativeBack` → `app::system_back`;
  on older versions Slint receives `Key.Back`. Back on Home moves the app to the background.
- The staff session token is kept in an Android Keystore-encrypted store (`SecureStore.kt`), and
  `allowBackup` is off so it never reaches a cloud backup.
- Errors (failed requests, panics) go to logcat with the tag `fwg`: `adb logcat -s fwg`.
- Icons come from `icon.png` (adaptive icon on `#060a10`, also used as the themed monochrome icon).
  `android/play-store-icon.png` is the 512 px store icon. The source is only 128 px, so the large sizes are
  upscaled; a larger original would make them sharper.

### Push notifications

Push goes straight through Firebase Cloud Messaging, with no Expo service in between.

- The Firebase client config belongs in `android/app/google-services.json` (not in git). Gradle turns it
  into Firebase's string resources itself instead of using the google-services plugin, so the `.dev` build
  shares the Play package's Firebase registration. Without the file the app builds with push disabled.
- "Enable notifications" asks for permission (Android 13+), fetches the FCM token and registers it with
  the website (`push/register.php`). Once enabled, the token is re-sent on every launch.
- `FwgMessagingService` shows messages while the app is open; otherwise FCM shows them (channel `default`).
  Tapping one opens `data.route` if it is on the allow-list in `crates/fwg-core/src/routes.rs`.
- Test routing without sending a push:
  `adb shell am start -n network.frontierwinggaming.app.dev/network.frontierwinggaming.app.MainActivity --es route /gallery`

### Release signing

Play uses Play App Signing; bundles are signed with the upload key that was registered for the Expo app.

1. Put the upload keystore in `android/` and create `android/keystore.properties` (both are ignored by git):
   ```
   storeFile=upload-keystore.jks
   storePassword=…
   keyAlias=…
   keyPassword=…
   ```
2. Look up the last versionCode in Play Console (App bundle explorer) and build with the next one.

Release builds stop at once if the key or the versionCode is missing. If the upload key is ever lost, Play
Console can register a new one (Setup → App signing → Request upload key reset).

The notes for each upload are kept in [PLAY_CHANGELOG.md](PLAY_CHANGELOG.md).

### 32-bit ARM

rust-skia publishes prebuilt Skia only for 64-bit Android, so for armeabi-v7a skia-bindings builds Skia
from source the first time (the result stays in `target/`). That needs:

- `ninja` and `python3` on `PATH`.
- libclang 19 or newer for bindgen, found through `LIBCLANG_PATH`. It must be at least the NDK's clang
  version; libclang 18 can't parse NDK 28's libc++.
- On Windows also:
  - A real `python3`: the name otherwise opens the Microsoft Store stub, so `build.ps1` puts a
    `python3.cmd` shim in `$SKIA_TOOLS\bin` first on `PATH`.
  - git long paths, since Skia's dependencies have very long file names.
  - bindgen's `-resource-dir` pointing at the NDK's clang headers, passed through
    `BINDGEN_EXTRA_CLANG_ARGS_armv7-linux-androideabi`. cargo-ndk overwrites the underscore form of that
    variable, but bindgen checks the hyphenated name first.

  `build.ps1` sets all three.

On Linux (and in CI) installing `ninja-build` and `libclang-19-dev` is enough.

## GitHub Actions

- `ci.yml` runs the `fwg-core` tests on every push and pull request. It uses no secrets.
- `play-release.yml` builds the signed bundle and uploads it to the Play **Internal testing** track. Run it
  from Actions → Play release → Run workflow and enter the next versionCode (untick the upload option to
  only build). The bundle is also kept as a run artifact for 30 days. The release's "What's new" text
  (en-US) is the ```` ```text ```` block under **Pending release** in [PLAY_CHANGELOG.md](PLAY_CHANGELOG.md);
  an upload stops before building if that block is empty or longer than Play's 500 characters.

The release secrets are stored in the repo's `play` environment, which only the `main` branch can use:

| Secret | Contents |
|---|---|
| `UPLOAD_KEYSTORE_BASE64` | the upload keystore, base64-encoded |
| `KEYSTORE_PROPERTIES` | the whole `keystore.properties` file (`storeFile=upload-keystore.jks`) |
| `GOOGLE_SERVICES_JSON` | the whole `google-services.json` |
| `PLAY_SERVICE_ACCOUNT_JSON` | JSON key of a Google Cloud service account invited in Play Console (Users and permissions) with release rights for this app |

Pull requests, including ones from forks, never get these secrets: only `ci.yml` runs for them.

## Slint 1.18 layout notes

Learned the hard way while building the screens; keep them in mind when adding UI:

- Wrapped text must sit **directly** in a card's vertical layout. Next to other items in a
  `HorizontalLayout`, or in a padded layout nested inside another element, it is measured at the wrong width
  and clipped. Draw icons, dots, arrows and avatars outside the layout (see `Card`'s `has-accent`, `has-dot`,
  `trailing`) or give the text an explicit `width` (see `MarkedRow`).
- Avoid `for` + `if` nesting around wrapped text; use one element type with property-driven variants
  (see `BlockView`).
- `StyledText` does not report its wrapped height; `RichText` sizes it from an invisible plain copy.
- Screens use `alignment: start` so short pages don't stretch their cards.
- Images are not clipped to rounded corners by the renderer; avatars are cut to a circle at decode time.

## Roadmap

1. ✅ Core library (`fwg-core`)
2. ✅ Public screens: Home, News, Live, Gallery, Servers, Server, About, More
3. ✅ Android app and staff screens: sign-in, publish news, send push with a confirm step, gallery upload
   (resized, EXIF/GPS data removed), manage news, gallery, staff and game servers, push history, change
   password
4. ✅ Push notifications through Firebase Cloud Messaging instead of Expo
5. iOS (needs a Mac or a cloud Mac)
