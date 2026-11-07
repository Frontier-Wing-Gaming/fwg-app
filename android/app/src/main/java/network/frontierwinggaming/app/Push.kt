package network.frontierwinggaming.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.graphics.Color
import android.util.Log
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage

/** Notification channel shared with the Expo app ("default", same name), so settings carry over. */
object Push {
    const val CHANNEL = "default"
    const val EXTRA_ROUTE = "route"
    private const val PREFS = "fwg.push"
    private const val KEY_ENABLED = "enabled"

    fun ensureChannel(context: Context) {
        val manager = context.getSystemService(NotificationManager::class.java)
        if (manager.getNotificationChannel(CHANNEL) == null) {
            manager.createNotificationChannel(
                NotificationChannel(CHANNEL, "Community updates", NotificationManager.IMPORTANCE_DEFAULT)
            )
        }
    }

    /** Whether the user turned notifications on in the app (re-register the token on launch). */
    fun isEnabled(context: Context) =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getBoolean(KEY_ENABLED, false)

    fun setEnabled(context: Context) =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putBoolean(KEY_ENABLED, true).apply()

    /** Opens the app on the notification's screen; the route is checked against an allow-list in Rust. */
    fun openAppIntent(context: Context, route: String?): PendingIntent {
        val intent = Intent(context, MainActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        if (route != null) intent.putExtra(EXTRA_ROUTE, route)
        return PendingIntent.getActivity(
            context, route.hashCode(), intent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
    }
}

/**
 * When the app is in the background, FCM shows notifications itself and a tap launches MainActivity
 * with the data fields (`route`) as intent extras. This service covers the app being open: FCM then
 * hands the message here instead of showing it.
 */
class FwgMessagingService : FirebaseMessagingService() {

    override fun onMessageReceived(message: RemoteMessage) {
        val title = message.notification?.title ?: message.data["title"] ?: return
        val body = message.notification?.body ?: message.data["body"] ?: ""
        Push.ensureChannel(this)
        val notification = Notification.Builder(this, Push.CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setColor(Color.parseColor("#ff5a1f"))
            .setContentTitle(title)
            .setContentText(body)
            .setStyle(Notification.BigTextStyle().bigText(body))
            .setAutoCancel(true)
            .setContentIntent(Push.openAppIntent(this, message.data[Push.EXTRA_ROUTE]))
            .build()
        try {
            getSystemService(NotificationManager::class.java)
                .notify(message.messageId?.hashCode() ?: System.currentTimeMillis().toInt(), notification)
        } catch (e: SecurityException) {
            Log.w("fwg", "Notification permission revoked", e)
        }
    }

    /** A rotated token is sent to the website the next time the app opens (see MainActivity). */
    @Deprecated("Replaced by onRegistered in firebase-messaging 26")
    override fun onNewToken(token: String) {
        Log.d("fwg", "FCM token rotated")
    }

    override fun onRegistered(token: String) {
        Log.d("fwg", "FCM token rotated")
    }
}
