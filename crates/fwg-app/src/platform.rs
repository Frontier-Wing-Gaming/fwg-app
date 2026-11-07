//! OS services: links, sharing, the data folder and leaving the app. Desktop versions use the
//! `open` and `arboard` crates; Android calls small helpers on `MainActivity` (Kotlin) over JNI.

use std::path::PathBuf;

/// Monospace font family for addresses and stats.
pub const MONO_FONT: &str = if cfg!(windows) {
    "Consolas"
} else if cfg!(target_os = "macos") {
    "Menlo"
} else {
    "monospace"
};

/// Label for the server page's share button.
pub const SHARE_LABEL: &str = if cfg!(any(target_os = "android", target_os = "ios")) {
    "Share address"
} else {
    "Copy address"
};

/// "Enable notifications". Returns a message to show now; `None` means the result arrives later
/// (Android: permission prompt + FCM token → `OsEvent::PushToken`).
#[cfg(not(target_os = "android"))]
pub fn enable_notifications() -> Option<&'static str> {
    Some("Push notifications are only available in the phone app.")
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub use desktop::*;

#[cfg(target_os = "android")]
pub use android::*;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod desktop {
    use super::PathBuf;
    use crate::app::{OsEvent, post_os_event};
    use fwg_core::storage::{KeyValueStore, MemoryStore};

    /// Where the cache, staff session and image cache live.
    pub fn data_dir() -> PathBuf {
        let base = std::env::var_os("LOCALAPPDATA")
            .or_else(|| std::env::var_os("XDG_DATA_HOME"))
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .unwrap_or_else(std::env::temp_dir);
        base.join("FrontierGamingNetwork")
    }

    /// Opens a web link or a game's connect link (`steam://…`, `fivem://…`) in the default handler.
    pub fn open_url(url: &str) -> Result<(), String> {
        open::that_detached(url).map_err(|e| e.to_string())
    }

    /// Shares text. On desktop it is copied; the returned message is shown to the user.
    pub fn share_text(text: &str) -> Result<&'static str, String> {
        let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        clipboard.set_text(text).map_err(|e| e.to_string())?;
        Ok("Address copied to the clipboard.")
    }

    /// Escape/back keys always count on desktop.
    pub fn handles_back_key() -> bool {
        true
    }

    /// Back pressed on the Home tab. A desktop window stays open.
    pub fn leave_app() {}

    /// Staff session storage in the OS credential store: Windows Credential Manager, or the
    /// Secret Service keyring (GNOME Keyring, KWallet) on Linux. Without a usable keyring the
    /// session is kept in memory only, so staff sign in again after restarting; it is never
    /// written to disk unencrypted.
    pub fn secure_store(data: &std::path::Path) -> std::sync::Arc<dyn KeyValueStore> {
        // Earlier development builds kept the session in plain files here; remove them.
        let _ = std::fs::remove_dir_all(data.join("secure"));
        match KeyringStore::probe() {
            Some(store) => std::sync::Arc::new(store),
            None => {
                log("No OS keyring available; the staff session will not be remembered.");
                std::sync::Arc::new(MemoryStore::default())
            }
        }
    }

    /// One credential per key, under the app's package name as the service.
    pub(super) struct KeyringStore;

    impl KeyringStore {
        const SERVICE: &'static str = if cfg!(feature = "e2e") {
            "network.frontierwinggaming.app.e2e"
        } else {
            "network.frontierwinggaming.app"
        };

        /// Returns a store if the keyring answers at all ("no entry" counts as working).
        pub(super) fn probe() -> Option<Self> {
            let entry = keyring::Entry::new(Self::SERVICE, "fwg.probe").ok()?;
            match entry.get_password() {
                Ok(_) | Err(keyring::Error::NoEntry) => Some(Self),
                Err(_) => None,
            }
        }

        fn entry(key: &str) -> std::io::Result<keyring::Entry> {
            keyring::Entry::new(Self::SERVICE, key).map_err(std::io::Error::other)
        }
    }

    impl KeyValueStore for KeyringStore {
        fn get(&self, key: &str) -> std::io::Result<Option<String>> {
            match Self::entry(key)?.get_password() {
                Ok(value) => Ok(Some(value)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(std::io::Error::other(e)),
            }
        }

        fn set(&self, key: &str, value: &str) -> std::io::Result<()> {
            Self::entry(key)?
                .set_password(value)
                .map_err(std::io::Error::other)
        }

        fn delete(&self, key: &str) -> std::io::Result<()> {
            match Self::entry(key)?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(std::io::Error::other(e)),
            }
        }
    }

    /// Gallery upload: a native file dialog; the result arrives as `OsEvent::ImagePicked`.
    pub fn pick_image() {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", &["jpg", "jpeg", "png", "webp", "gif"])
            .pick_file()
        else {
            return;
        };
        let event = match std::fs::read(&path) {
            Ok(bytes) => OsEvent::ImagePicked {
                bytes: Some(bytes),
                name: path.file_name().map(|n| n.to_string_lossy().into_owned()),
                mime: None,
                error: None,
            },
            Err(e) => OsEvent::ImagePicked {
                bytes: None,
                name: None,
                mime: None,
                error: Some(format!("The image could not be read: {e}")),
            },
        };
        post_os_event(event);
    }

    /// Diagnostics go to stderr (visible when run from a terminal).
    pub fn log(message: &str) {
        eprintln!("{message}");
    }
}

#[cfg(target_os = "android")]
mod android {
    use super::PathBuf;
    use std::{
        io,
        sync::{Arc, OnceLock},
    };

    use fwg_core::storage::KeyValueStore;
    use jni::{
        JNIEnv, JavaVM,
        objects::{JObject, JString, JValue},
    };
    use slint::android::AndroidApp;

    static APP: OnceLock<AndroidApp> = OnceLock::new();

    /// Called once from `android_main`, before anything else here is used.
    pub fn init(app: AndroidApp) {
        let _ = APP.set(app);
    }

    /// Whether Slint should treat KEYCODE_BACK as back. From Android 13 (API 33) MainActivity
    /// registers an OnBackInvokedCallback that calls `system_back` instead.
    pub fn handles_back_key() -> bool {
        APP.get().is_none_or(|app| app.config().sdk_version() < 33)
    }

    /// The app's private storage (`/data/data/<package>/files`).
    pub fn data_dir() -> PathBuf {
        APP.get()
            .and_then(|app| app.internal_data_path())
            .unwrap_or_else(std::env::temp_dir)
    }

    /// Opens a link through an ACTION_VIEW intent (browser, Discord, Steam, …).
    pub fn open_url(url: &str) -> Result<(), String> {
        match call_activity("openUrl", "(Ljava/lang/String;)Z", Some(url))? {
            true => Ok(()),
            false => Err("No app can open this link.".into()),
        }
    }

    /// Opens the Android share sheet.
    pub fn share_text(text: &str) -> Result<&'static str, String> {
        match call_activity("shareText", "(Ljava/lang/String;)Z", Some(text))? {
            // The share sheet is its own feedback; no notice under the button.
            true => Ok(""),
            false => Err("Sharing is not available.".into()),
        }
    }

    /// Asks MainActivity for notification permission and an FCM token (answer: nativePushResult).
    pub fn enable_notifications() -> Option<&'static str> {
        match call_activity("enableNotifications", "()Z", None) {
            Ok(_) => None,
            Err(e) => {
                log(&format!("enableNotifications failed: {e}"));
                Some("Notifications could not be enabled.")
            }
        }
    }

    /// Back pressed on the Home tab: send the app to the background like other Android apps.
    pub fn leave_app() {
        if let Err(e) = call_activity("leaveApp", "()Z", None) {
            log(&format!("leaveApp failed: {e}"));
        }
    }

    /// Gallery upload: the system photo picker; the result arrives as `OsEvent::ImagePicked`.
    pub fn pick_image() {
        if let Err(e) = call_activity("pickImage", "()Z", None) {
            log(&format!("pickImage failed: {e}"));
        }
    }

    /// Staff session storage, encrypted with a Keystore key (MainActivity → SecureStore.kt).
    pub fn secure_store(_data: &std::path::Path) -> Arc<dyn KeyValueStore> {
        Arc::new(AndroidSecureStore)
    }

    struct AndroidSecureStore;

    fn io_err(e: impl std::fmt::Display) -> io::Error {
        io::Error::other(e.to_string())
    }

    impl KeyValueStore for AndroidSecureStore {
        fn get(&self, key: &str) -> io::Result<Option<String>> {
            with_activity(|env, activity| {
                let key = env.new_string(key)?;
                let value = env
                    .call_method(
                        activity,
                        "secureGet",
                        "(Ljava/lang/String;)Ljava/lang/String;",
                        &[JValue::Object(&key)],
                    )?
                    .l()?;
                if value.is_null() {
                    return Ok(None);
                }
                Ok(Some(env.get_string(&JString::from(value))?.into()))
            })
            .map_err(io_err)
        }

        fn set(&self, key: &str, value: &str) -> io::Result<()> {
            let ok = with_activity(|env, activity| {
                let key = env.new_string(key)?;
                let value = env.new_string(value)?;
                env.call_method(
                    activity,
                    "secureSet",
                    "(Ljava/lang/String;Ljava/lang/String;)Z",
                    &[JValue::Object(&key), JValue::Object(&value)],
                )?
                .z()
            })
            .map_err(io_err)?;
            if ok {
                Ok(())
            } else {
                Err(io_err("secure write failed"))
            }
        }

        fn delete(&self, key: &str) -> io::Result<()> {
            with_activity(|env, activity| {
                let key = env.new_string(key)?;
                env.call_method(
                    activity,
                    "secureDelete",
                    "(Ljava/lang/String;)Z",
                    &[JValue::Object(&key)],
                )?
                .z()
            })
            .map(|_| ())
            .map_err(io_err)
        }
    }

    /// Calls a `MainActivity` method that takes an optional String and returns a boolean.
    fn call_activity(method: &str, signature: &str, arg: Option<&str>) -> Result<bool, String> {
        with_activity(|env, activity| match arg {
            Some(arg) => {
                let arg = env.new_string(arg)?;
                env.call_method(activity, method, signature, &[JValue::Object(&arg)])?
                    .z()
            }
            None => env.call_method(activity, method, signature, &[])?.z(),
        })
    }

    /// Runs JNI code against the activity on the current thread (attaching it if needed). The
    /// Kotlin side catches its own exceptions; anything that still escapes is cleared here.
    pub(crate) fn with_activity<R>(
        f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<R>,
    ) -> Result<R, String> {
        let app = APP.get().ok_or("Android activity is not ready.")?;
        // SAFETY: both pointers come from android-activity and stay valid for the app's lifetime.
        let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }.map_err(|e| e.to_string())?;
        let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
        let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
        let result = f(&mut env, &activity);
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_describe();
            let _ = env.exception_clear();
        }
        result.map_err(|e| e.to_string())
    }

    /// Writes a line to logcat under the tag "fwg".
    pub fn log(message: &str) {
        unsafe extern "C" {
            fn __android_log_write(prio: i32, tag: *const u8, text: *const u8) -> i32;
        }
        let text = std::ffi::CString::new(message.replace('\0', " ")).unwrap_or_default();
        // 6 = ANDROID_LOG_ERROR
        unsafe { __android_log_write(6, c"fwg".as_ptr().cast(), text.as_ptr().cast()) };
    }
}

#[cfg(all(test, not(any(target_os = "android", target_os = "ios"))))]
mod tests {
    use fwg_core::storage::KeyValueStore;

    /// Touches the real OS keyring, so it only runs when asked: `cargo test -- --ignored keyring`.
    #[test]
    #[ignore]
    fn keyring_round_trip() {
        let store = super::desktop::KeyringStore::probe().expect("no keyring");
        let key = "fwg.test.round-trip";
        store.set(key, "value ✓").unwrap();
        assert_eq!(store.get(key).unwrap().as_deref(), Some("value ✓"));
        store.delete(key).unwrap();
        assert_eq!(store.get(key).unwrap(), None);
        store.delete(key).unwrap(); // deleting a missing key is fine
    }
}
