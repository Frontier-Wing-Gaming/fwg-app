//! Frontier Gaming Network app: Slint UI on top of `fwg-core`.

slint::include_modules!();

#[cfg(target_os = "android")]
mod android_a11y;
mod app;
mod images;
mod platform;
mod view;

use std::sync::Arc;

use fwg_core::{
    ApiClient, Cache,
    storage::{FileStore, KeyValueStore, MemoryStore},
};

/// Android entry point, called by NativeActivity (see `android/` for the Gradle project).
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    std::panic::set_hook(Box::new(|info| platform::log(&format!("panic: {info}"))));
    platform::init(app.clone());
    if let Err(e) = slint::android::init(app) {
        platform::log(&format!("could not start Slint: {e}"));
        return;
    }
    if let Err(e) = run() {
        platform::log(&format!("app stopped: {e}"));
    }
}

// ---- JNI entry points called by MainActivity (Java UI thread); they only queue an OsEvent.

/// `MainActivity.nativeBack()`: the Android 13+ back button/gesture.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
extern "system" fn Java_network_frontierwinggaming_app_MainActivity_nativeBack(
    _env: jni::JNIEnv,
    _activity: jni::objects::JObject,
) {
    app::post_os_event(app::OsEvent::Back);
}

/// `MainActivity.nativeRoute(route)`: a tapped notification's `data.route`.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
extern "system" fn Java_network_frontierwinggaming_app_MainActivity_nativeRoute(
    mut env: jni::JNIEnv,
    _activity: jni::objects::JObject,
    route: jni::objects::JString,
) {
    if let Some(route) = java_string(&mut env, &route) {
        app::post_os_event(app::OsEvent::Route(route));
    }
}

/// `MainActivity.nativePushResult(token, error, silent)`: outcome of fetching the FCM token.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
extern "system" fn Java_network_frontierwinggaming_app_MainActivity_nativePushResult(
    mut env: jni::JNIEnv,
    _activity: jni::objects::JObject,
    token: jni::objects::JString,
    error: jni::objects::JString,
    silent: jni::sys::jboolean,
) {
    let token = java_string(&mut env, &token);
    let error = java_string(&mut env, &error);
    app::post_os_event(app::OsEvent::PushToken {
        token,
        error,
        silent: silent != 0,
    });
}

/// `MainActivity.nativeImagePicked(bytes, name, mime, error)`: the photo picker's result.
/// `bytes` is null when nothing was picked or reading failed (`error` then says why).
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
extern "system" fn Java_network_frontierwinggaming_app_MainActivity_nativeImagePicked(
    mut env: jni::JNIEnv,
    _activity: jni::objects::JObject,
    bytes: jni::objects::JByteArray,
    name: jni::objects::JString,
    mime: jni::objects::JString,
    error: jni::objects::JString,
) {
    let bytes = if bytes.is_null() {
        None
    } else {
        env.convert_byte_array(&bytes).ok()
    };
    let name = java_string(&mut env, &name);
    let mime = java_string(&mut env, &mime);
    let error = java_string(&mut env, &error);
    app::post_os_event(app::OsEvent::ImagePicked {
        bytes,
        name,
        mime,
        error,
    });
}

#[cfg(target_os = "android")]
fn java_string(env: &mut jni::JNIEnv, value: &jni::objects::JString) -> Option<String> {
    if value.is_null() {
        return None;
    }
    env.get_string(value).ok().map(String::from)
}

/// Builds the window and runs the event loop until it closes.
pub fn run() -> Result<(), slint::PlatformError> {
    // One worker is plenty for a handful of HTTP requests; a small blocking pool decodes images.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .max_blocking_threads(2)
        .thread_name("fwg-io")
        .enable_all()
        .build()
        .map_err(|e| slint::PlatformError::Other(e.to_string()))?;

    // The software renderer measured ~32 MB vs ~96 MB for OpenGL (femtovg) on the same tour of
    // every screen; most of the difference is GPU driver overhead. SLINT_BACKEND overrides it,
    // e.g. `SLINT_BACKEND=winit-femtovg` to compare. Android has its own backend (Skia), set up
    // in `android_main`.
    #[cfg(not(target_os = "android"))]
    if std::env::var_os("SLINT_BACKEND").is_none() {
        slint::BackendSelector::new()
            .backend_name("winit".into())
            .renderer_name("software".into())
            .select()?;
    }
    // Linux desktops match the window to network.frontierwinggaming.app.desktop (menu entry
    // and icon) by this id.
    #[cfg(target_os = "linux")]
    slint::set_xdg_app_id("network.frontierwinggaming.app")?;

    let ui = AppWindow::new()?;
    // End-to-end tests on GNOME and KDE start the window maximized, so its position on screen
    // (which Wayland doesn't tell clients) is known.
    #[cfg(feature = "e2e")]
    if std::env::var_os("FWG_E2E_MAXIMIZED").is_some() {
        ui.window().set_maximized(true);
    }
    ui.global::<Fonts>().set_mono(platform::MONO_FONT.into());
    ui.global::<AppState>()
        .set_back_key(platform::handles_back_key());

    let data = platform::data_dir();
    let store: Arc<dyn KeyValueStore> = match FileStore::new(data.join("store")) {
        Ok(store) => Arc::new(store),
        // Still usable without disk; there is just no offline copy.
        Err(_) => Arc::new(MemoryStore::default()),
    };
    let api = api_client();
    let images =
        images::ImageLoader::new(api.http().clone(), rt.handle().clone(), data.join("images"));
    let session = fwg_core::Session::restore(api.clone(), platform::secure_store(&data));
    let app = app::App::new(
        &ui,
        rt.handle().clone(),
        api,
        session,
        Cache::new(store),
        images,
    );
    app.start();
    #[cfg(target_os = "android")]
    android_a11y::install(&ui);

    let result = ui.run();
    drop(app);
    rt.shutdown_background();
    result
}

/// The website API. End-to-end test builds (`--features e2e`) talk to the mock server named in
/// `FWG_API_ROOT` instead.
fn api_client() -> ApiClient {
    #[cfg(feature = "e2e")]
    if let Some(root) = std::env::var_os("FWG_API_ROOT") {
        return ApiClient::new(&root.to_string_lossy());
    }
    ApiClient::default()
}
