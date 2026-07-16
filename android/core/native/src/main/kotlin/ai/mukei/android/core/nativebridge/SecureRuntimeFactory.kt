package ai.mukei.android.core.nativebridge

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.io.File
import java.io.FileOutputStream
import java.nio.ByteBuffer
import java.nio.charset.StandardCharsets
import java.nio.file.AtomicMoveNotSupportedException
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.security.KeyStore
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import org.json.JSONObject

/**
 * Creates a production runtime backed by SQLCipher and Android Keystore.
 *
 * The raw 32-byte database key exists only in short-lived byte arrays. At rest,
 * only an AES-GCM envelope is stored below app-private files storage. The AES
 * wrapping key is non-exportable and remains inside Android Keystore.
 */
object SecureRuntimeFactory {
    /** Opens or creates the wrapped database key and constructs the secure JNI runtime. */
    @Synchronized
    fun open(
        context: Context,
        configJson: ByteArray,
    ): RustNativeGateway {
        require(configJson.isNotEmpty()) { "Runtime configuration must not be empty" }
        val appContext = context.applicationContext
        val filesRoot = appContext.filesDir.canonicalFile
        val configuredRoot = try {
            val config = JSONObject(String(configJson, StandardCharsets.UTF_8))
            File(config.getString("app_data_dir")).canonicalFile
        } catch (failure: Exception) {
            throw SecurityBootstrapException("runtime_config_invalid", failure)
        }
        ensureInsideFilesDir(configuredRoot, filesRoot, allowRoot = true)

        val keyFile = File(filesRoot, DATABASE_KEY_FILE).canonicalFile
        ensureInsideFilesDir(keyFile, filesRoot)
        keyFile.parentFile?.let { parent ->
            if (!parent.exists() && !parent.mkdirs()) {
                throw SecurityBootstrapException("secure_storage_unavailable")
            }
        }

        val rawKey = if (keyFile.exists()) {
            unwrapDatabaseKey(keyFile)
        } else {
            createAndPersistDatabaseKey(keyFile)
        }
        return try {
            RustNativeGateway.createSecure(configJson, rawKey)
        } finally {
            rawKey.fill(0)
        }
    }

    private fun createAndPersistDatabaseKey(keyFile: File): ByteArray {
        val rawKey = ByteArray(DATABASE_KEY_BYTES).also(SecureRandom()::nextBytes)
        return try {
            val wrapped = wrap(rawKey)
            try {
                writeAtomically(keyFile, wrapped)
            } finally {
                wrapped.fill(0)
            }
            rawKey
        } catch (failure: Exception) {
            rawKey.fill(0)
            if (failure is SecurityBootstrapException) throw failure
            throw SecurityBootstrapException("database_key_creation_failed", failure)
        }
    }

    private fun unwrapDatabaseKey(keyFile: File): ByteArray {
        val wrapped = try {
            Files.readAllBytes(keyFile.toPath())
        } catch (failure: Exception) {
            throw SecurityBootstrapException("database_key_read_failed", failure)
        }
        if (wrapped.size !in MIN_ENVELOPE_BYTES..MAX_ENVELOPE_BYTES) {
            wrapped.fill(0)
            throw SecurityBootstrapException("database_key_envelope_invalid")
        }
        return try {
            unwrap(wrapped).also { raw ->
                if (raw.size != DATABASE_KEY_BYTES) {
                    raw.fill(0)
                    throw SecurityBootstrapException("database_key_length_invalid")
                }
            }
        } finally {
            wrapped.fill(0)
        }
    }

    private fun wrap(rawKey: ByteArray): ByteArray {
        val cipher = Cipher.getInstance(KEY_TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, getOrCreateWrappingKey())
        val ciphertext = cipher.doFinal(rawKey)
        val iv = cipher.iv
        if (iv.size !in MIN_GCM_IV_BYTES..MAX_GCM_IV_BYTES) {
            ciphertext.fill(0)
            throw SecurityBootstrapException("keystore_invalid_iv")
        }
        return try {
            ByteBuffer.allocate(2 + iv.size + ciphertext.size)
                .put(ENVELOPE_VERSION)
                .put(iv.size.toByte())
                .put(iv)
                .put(ciphertext)
                .array()
        } finally {
            iv.fill(0)
            ciphertext.fill(0)
        }
    }

    private fun unwrap(envelope: ByteArray): ByteArray {
        if (envelope.size < MIN_ENVELOPE_BYTES || envelope[0] != ENVELOPE_VERSION) {
            throw SecurityBootstrapException("database_key_envelope_invalid")
        }
        val ivSize = envelope[1].toInt() and 0xff
        if (ivSize !in MIN_GCM_IV_BYTES..MAX_GCM_IV_BYTES || envelope.size <= 2 + ivSize) {
            throw SecurityBootstrapException("database_key_envelope_invalid")
        }
        val iv = envelope.copyOfRange(2, 2 + ivSize)
        val ciphertext = envelope.copyOfRange(2 + ivSize, envelope.size)
        return try {
            val keyStore = androidKeyStore()
            val wrappingKey = keyStore.getKey(WRAP_ALIAS, null) as? SecretKey
                ?: throw SecurityBootstrapException("keystore_key_missing")
            Cipher.getInstance(KEY_TRANSFORMATION).run {
                init(Cipher.DECRYPT_MODE, wrappingKey, GCMParameterSpec(GCM_TAG_BITS, iv))
                doFinal(ciphertext)
            }
        } catch (failure: SecurityBootstrapException) {
            throw failure
        } catch (failure: Exception) {
            throw SecurityBootstrapException("database_key_unwrap_failed", failure)
        } finally {
            iv.fill(0)
            ciphertext.fill(0)
        }
    }

    private fun getOrCreateWrappingKey(): SecretKey {
        val keyStore = androidKeyStore()
        (keyStore.getKey(WRAP_ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(
            KeyProperties.KEY_ALGORITHM_AES,
            ANDROID_KEYSTORE,
        )
        generator.init(
            KeyGenParameterSpec.Builder(
                WRAP_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return generator.generateKey()
    }

    private fun androidKeyStore(): KeyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply {
        load(null)
    }

    private fun writeAtomically(target: File, bytes: ByteArray) {
        val parent = target.parentFile
            ?: throw SecurityBootstrapException("secure_storage_unavailable")
        val temporary = File(parent, ".${target.name}.partial").canonicalFile
        ensureInsideFilesDir(
            temporary,
            parent.parentFile?.canonicalFile ?: parent.canonicalFile,
        )
        try {
            FileOutputStream(temporary).use { output ->
                output.write(bytes)
                output.fd.sync()
            }
            try {
                Files.move(
                    temporary.toPath(),
                    target.toPath(),
                    StandardCopyOption.ATOMIC_MOVE,
                )
            } catch (_: AtomicMoveNotSupportedException) {
                Files.move(
                    temporary.toPath(),
                    target.toPath(),
                    StandardCopyOption.REPLACE_EXISTING,
                )
            }
        } catch (failure: Exception) {
            temporary.delete()
            throw SecurityBootstrapException("database_key_persist_failed", failure)
        }
    }

    private fun ensureInsideFilesDir(
        target: File,
        filesDir: File,
        allowRoot: Boolean = false,
    ) {
        if (!target.toPath().startsWith(filesDir.toPath()) || (!allowRoot && target == filesDir)) {
            throw SecurityBootstrapException("secure_storage_path_rejected")
        }
    }

    class SecurityBootstrapException(
        val code: String,
        cause: Throwable? = null,
    ) : IllegalStateException(code, cause)

    private const val ANDROID_KEYSTORE = "AndroidKeyStore"
    private const val WRAP_ALIAS = "mukei.database.wrap.v1"
    private const val DATABASE_KEY_FILE = "mukei/secrets/db_key.enc"
    private const val DATABASE_KEY_BYTES = 32
    private const val KEY_TRANSFORMATION = "AES/GCM/NoPadding"
    private const val GCM_TAG_BITS = 128
    private const val MIN_GCM_IV_BYTES = 12
    private const val MAX_GCM_IV_BYTES = 32
    private const val ENVELOPE_VERSION: Byte = 1
    private const val MIN_ENVELOPE_BYTES = 3
    private const val MAX_ENVELOPE_BYTES = 512
}
