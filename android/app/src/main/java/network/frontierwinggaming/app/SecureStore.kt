package network.frontierwinggaming.app

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import android.util.Log
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Small encrypted key-value store for the staff session (token + user), the Rust side's
 * "secure store". Values are AES-256-GCM encrypted with a key that lives in the Android Keystore
 * (never leaves secure hardware where available) and stored as base64 in private SharedPreferences.
 * If the key is lost (e.g. restored onto another device) values fail to decrypt and read as absent,
 * which just means signing in again.
 */
class SecureStore(context: Context) {
    private val prefs = context.getSharedPreferences("fwg.secure", Context.MODE_PRIVATE)

    @Synchronized
    fun get(key: String): String? {
        val stored = prefs.getString(key, null) ?: return null
        return try {
            val data = Base64.decode(stored, Base64.NO_WRAP)
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, data, 0, IV_BYTES))
            String(cipher.doFinal(data, IV_BYTES, data.size - IV_BYTES), Charsets.UTF_8)
        } catch (e: Exception) {
            Log.w("fwg", "Secure value unreadable; dropping it", e)
            prefs.edit().remove(key).apply()
            null
        }
    }

    @Synchronized
    fun set(key: String, value: String): Boolean = try {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val sealed = cipher.iv + cipher.doFinal(value.toByteArray(Charsets.UTF_8))
        prefs.edit().putString(key, Base64.encodeToString(sealed, Base64.NO_WRAP)).commit()
    } catch (e: Exception) {
        Log.w("fwg", "Secure write failed", e)
        false
    }

    @Synchronized
    fun delete(key: String): Boolean = prefs.edit().remove(key).commit()

    private fun key(): SecretKey {
        val keyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (keyStore.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        generator.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build()
        )
        return generator.generateKey()
    }

    private companion object {
        const val ALIAS = "fwg.secure"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val IV_BYTES = 12
    }
}
