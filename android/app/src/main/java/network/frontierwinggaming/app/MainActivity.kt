package network.frontierwinggaming.app

import android.Manifest
import android.app.NativeActivity
import android.app.NotificationManager
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.MediaStore
import android.provider.OpenableColumns
import android.util.Log
import android.view.View
import android.view.ViewGroup
import android.view.accessibility.AccessibilityManager
import android.window.OnBackInvokedDispatcher
import com.google.firebase.messaging.FirebaseMessaging

/**
 * Hosts the Rust UI (libfwg_app.so). Rust calls the small helpers below over JNI for things only
 * the Android framework can do. Each returns false instead of throwing, so a missing app (e.g. no
 * Steam client for a steam:// link) becomes a message in the UI rather than a crash.
 *
 * Calls into Rust (`native*`) may arrive before the Rust event loop runs; Rust queues them.
 */
class MainActivity : NativeActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Draw behind the status and navigation bars; Slint pads the UI with the safe-area insets.
        // (Deprecated on 35+, where edge-to-edge is the default; still needed for Android 8–14.)
        @Suppress("DEPRECATION")
        window.statusBarColor = Color.TRANSPARENT
        @Suppress("DEPRECATION")
        window.navigationBarColor = Color.TRANSPARENT
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            @Suppress("DEPRECATION")
            window.setDecorFitsSystemWindows(false)
        } else {
            @Suppress("DEPRECATION")
            window.decorView.systemUiVisibility = View.SYSTEM_UI_FLAG_LAYOUT_STABLE or
                View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN or
                View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
        }
        // Android 13+ delivers back (button or gesture) here instead of as a key event; Rust decides
        // whether that pops a screen, goes Home or leaves the app. Older versions send KEYCODE_BACK
        // to the native input queue, which Slint maps to Key.Back.
        // PRIORITY_OVERLAY: Slint's text-input view registers its own default-priority callback
        // after this one, and the last registered callback of equal priority wins.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            onBackInvokedDispatcher.registerOnBackInvokedCallback(
                OnBackInvokedDispatcher.PRIORITY_OVERLAY
            ) { nativeBack() }
        }

        Push.ensureChannel(this)
        deliverRoute(intent)
        // Keep the website's copy of this device's token current (FCM rotates tokens now and then).
        if (Push.isEnabled(this) && notificationsAllowed()) fetchToken(silent = true)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        deliverRoute(intent)
    }

    /** A tapped notification carries the screen to open (`route`, e.g. "/news"). */
    private fun deliverRoute(intent: Intent?) {
        val route = intent?.getStringExtra(Push.EXTRA_ROUTE) ?: return
        intent.removeExtra(Push.EXTRA_ROUTE)
        nativeRoute(route)
    }

    // ------------------------------------------------------------------ called from Rust

    /** Opens a web or game link (https, steam://, fivem://, discord) in whatever app handles it. */
    @Suppress("unused")
    fun openUrl(url: String): Boolean = try {
        startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        true
    } catch (e: Exception) {
        Log.w(TAG, "No handler for $url", e)
        false
    }

    /** Shows the system share sheet with plain text. */
    @Suppress("unused")
    fun shareText(text: String): Boolean = try {
        val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, text)
        startActivity(Intent.createChooser(send, null).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        true
    } catch (e: Exception) {
        Log.w(TAG, "Share failed", e)
        false
    }

    /** Back on the Home tab: move to the background like other apps (keeps the app warm). */
    @Suppress("unused")
    fun leaveApp(): Boolean {
        runOnUiThread { moveTaskToBack(true) }
        return true
    }

    /**
     * "Enable notifications": asks for permission if needed, then fetches the FCM token. The result
     * arrives in Rust through `nativePushResult`.
     */
    @Suppress("unused")
    fun enableNotifications(): Boolean {
        runOnUiThread {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
                checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
            ) {
                requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), REQUEST_NOTIFICATIONS)
            } else if (!notificationsAllowed()) {
                nativePushResult(null, "Notifications are turned off for this app in Android settings.", false)
            } else {
                fetchToken(silent = false)
            }
        }
        return true
    }

    @Deprecated("Platform activity API; NativeActivity has no ActivityResult support")
    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode != REQUEST_NOTIFICATIONS) return
        if (grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED) {
            fetchToken(silent = false)
        } else {
            nativePushResult(null, "Notification permission was not granted.", false)
        }
    }

    /**
     * Screen readers: the view that Rust's AccessKit adapter (src/android_a11y.rs) attaches to,
     * Slint's full-screen input view, which sits on top of NativeActivity's own view. Null until
     * Slint has added it. The other views under android.R.id.content have nothing to say, so
     * TalkBack is told to skip them. Called from the Rust thread; only reads the hierarchy.
     */
    @Suppress("unused")
    fun accessibilityHost(): View? {
        val content = findViewById<ViewGroup>(android.R.id.content) ?: return null
        val children = (0 until content.childCount).map { content.getChildAt(it) }
        val host = children.lastOrNull { it.javaClass.simpleName == "SlintInputView" } ?: return null
        runOnUiThread {
            for (child in children) {
                if (child !== host) child.importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
            }
        }
        return host
    }

    /** Whether a screen reader (or another service that reads the screen) is on. Any thread. */
    @Suppress("unused")
    fun screenReaderOn(): Boolean =
        getSystemService(AccessibilityManager::class.java)?.isEnabled == true

    // Staff session storage (see SecureStore). Called from Rust on any thread.
    private val secureStore by lazy { SecureStore(applicationContext) }

    @Suppress("unused")
    fun secureGet(key: String): String? = secureStore.get(key)

    @Suppress("unused")
    fun secureSet(key: String, value: String): Boolean = secureStore.set(key, value)

    @Suppress("unused")
    fun secureDelete(key: String): Boolean = secureStore.delete(key)

    /**
     * Gallery upload: opens the system photo picker (no storage permission needed). The chosen
     * file's bytes go to Rust through `nativeImagePicked`; null bytes mean cancelled or failed.
     */
    @Suppress("unused")
    fun pickImage(): Boolean {
        runOnUiThread {
            val intent = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                Intent(MediaStore.ACTION_PICK_IMAGES).setType("image/*")
            } else {
                Intent(Intent.ACTION_GET_CONTENT).setType("image/*").addCategory(Intent.CATEGORY_OPENABLE)
            }
            try {
                @Suppress("DEPRECATION")
                startActivityForResult(intent, REQUEST_IMAGE)
            } catch (e: Exception) {
                Log.w(TAG, "No image picker", e)
                nativeImagePicked(null, null, null, "No photo picker is available on this device.")
            }
        }
        return true
    }

    @Deprecated("Platform activity API; NativeActivity has no ActivityResult support")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        @Suppress("DEPRECATION")
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != REQUEST_IMAGE) return
        val uri = data?.data
        if (resultCode != RESULT_OK || uri == null) {
            nativeImagePicked(null, null, null, null) // cancelled
            return
        }
        // Read off the UI thread; photos can be several MB.
        Thread {
            try {
                var name: String? = null
                var size = -1L
                contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)
                    ?.use { if (it.moveToFirst()) { name = it.getString(0); size = it.getLong(1) } }
                if (size > MAX_PICK_BYTES) {
                    nativeImagePicked(null, null, null, "That file is too large (over 40 MB).")
                    return@Thread
                }
                val bytes = contentResolver.openInputStream(uri)?.use { it.readBytes() }
                    ?: throw IllegalStateException("unreadable")
                nativeImagePicked(bytes, name, contentResolver.getType(uri), null)
            } catch (e: Exception) {
                Log.w(TAG, "Reading picked image failed", e)
                nativeImagePicked(null, null, null, "The image could not be read.")
            }
        }.start()
    }

    // ------------------------------------------------------------------ helpers

    private fun notificationsAllowed(): Boolean =
        getSystemService(NotificationManager::class.java).areNotificationsEnabled()

    private fun fetchToken(silent: Boolean) {
        val messaging = try {
            FirebaseMessaging.getInstance()
        } catch (e: Exception) {
            Log.w(TAG, "Firebase is not configured", e)
            nativePushResult(null, "Notifications are not included in this build of the app.", silent)
            return
        }
        // firebase-messaging 26 deprecates getToken() in favour of register() + the service's
        // onRegistered(token). getToken() still works and answers right here; migrate when it is
        // removed.
        @Suppress("DEPRECATION")
        messaging.token.addOnCompleteListener { task ->
            val token = if (task.isSuccessful) task.result else null
            if (token.isNullOrEmpty()) {
                Log.w(TAG, "FCM token failed", task.exception)
                nativePushResult(null, "Could not connect to the notification service. Try again later.", silent)
            } else {
                Push.setEnabled(this)
                nativePushResult(token, null, silent)
            }
        }
    }

    private external fun nativeBack()
    private external fun nativeRoute(route: String)
    private external fun nativePushResult(token: String?, error: String?, silent: Boolean)
    private external fun nativeImagePicked(bytes: ByteArray?, name: String?, mime: String?, error: String?)

    private companion object {
        const val TAG = "fwg"
        const val REQUEST_NOTIFICATIONS = 1
        const val REQUEST_IMAGE = 2
        const val MAX_PICK_BYTES = 40L * 1024 * 1024

        init {
            // NativeActivity dlopens the library itself; loading it here as well registers it with
            // this class loader so the `external` functions above resolve (same handle, no re-init).
            System.loadLibrary("fwg_app")
        }
    }
}
