package ai.mukei.android.core.nativebridge

import android.app.ActivityManager
import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.Uri
import android.os.Build
import android.os.PowerManager
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.io.FileOutputStream
import java.nio.ByteBuffer
import java.nio.charset.StandardCharsets
import java.nio.file.AtomicMoveNotSupportedException
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.security.KeyStore
import java.security.MessageDigest
import java.util.UUID
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Executes Android-only requests emitted by [MukeiNativeGateway].
 *
 * Run [processOnce] from a repository-owned worker thread. Rust never calls
 * Java from inference/download threads; Kotlin explicitly drains this queue and
 * returns one bounded response per request.
 */
class AndroidPlatformRequestProcessor(
    context: Context,
    private val gateway: MukeiNativeGateway,
) {
    private val appContext = context.applicationContext
    private val documentRoot = File(appContext.filesDir, "mukei/documents")

    data class BatchResult(
        val processed: Int,
        val hasMore: Boolean,
    )

    /** Drains and executes one bounded request batch. */
    fun processOnce(
        maximumRequests: Int = 8,
        timeoutMilliseconds: Long = 0,
    ): BatchResult {
        val batchBytes = gateway.drainPlatformRequests(maximumRequests, timeoutMilliseconds)
        val batch = JSONObject(String(batchBytes, StandardCharsets.UTF_8))
        batch.optJSONObject("error")?.let { error ->
            throw IllegalStateException(error.optString("code", "platform_drain_failed"))
        }
        val requests = batch.optJSONArray("requests") ?: JSONArray()
        for (index in 0 until requests.length()) {
            val envelope = requests.getJSONObject(index)
            val requestId = envelope.getString("request_id")
            val request = envelope.getJSONObject("request")
            val response = try {
                successResponse(requestId, execute(request))
            } catch (failure: PlatformFailure) {
                failureResponse(requestId, failure.code, failure.safeMessage)
            } catch (failure: SecurityException) {
                failureResponse(requestId, "android_permission_denied", "Android denied the operation")
            } catch (failure: Exception) {
                failureResponse(
                    requestId,
                    "android_platform_failure",
                    failure.javaClass.simpleName.ifBlank { "PlatformFailure" },
                )
            }
            val receiptBytes = gateway.submitPlatformResponse(
                response.toString().toByteArray(StandardCharsets.UTF_8),
            )
            val receipt = JSONObject(String(receiptBytes, StandardCharsets.UTF_8))
            receipt.optJSONObject("error")?.let { error ->
                throw IllegalStateException(error.optString("code", "platform_response_rejected"))
            }
        }
        return BatchResult(
            processed = requests.length(),
            hasMore = batch.optBoolean("has_more", false),
        )
    }

    private fun execute(request: JSONObject): JSONObject = when (request.getString("kind")) {
        "document_stage" -> stageDocument(request)
        "document_delete" -> deleteDocument(request)
        "secure_key_wrap" -> wrapSecret(request)
        "secure_key_unwrap" -> unwrapSecret(request)
        "thermal_status" -> thermalStatus()
        "network_status" -> networkStatus()
        "diagnostics_snapshot" -> diagnosticsSnapshot()
        else -> throw PlatformFailure("unsupported_platform_request", "Unsupported platform request")
    }

    private fun stageDocument(request: JSONObject): JSONObject {
        val target = request.getString("target")
        val uri = Uri.parse(target)
        if (uri.scheme != "content") {
            throw PlatformFailure("invalid_document_uri", "Only content URIs are accepted")
        }
        val root = documentRoot.canonicalFile
        if (!root.exists() && !root.mkdirs()) {
            throw PlatformFailure("document_storage_unavailable", "Document storage is unavailable")
        }
        val temporary = File(root, ".${UUID.randomUUID()}.partial").canonicalFile
        val destination = File(root, "${UUID.randomUUID()}.blob").canonicalFile
        ensureInsideDocumentRoot(temporary, root)
        ensureInsideDocumentRoot(destination, root)

        val digest = MessageDigest.getInstance("SHA-256")
        var total = 0L
        try {
            val input = appContext.contentResolver.openInputStream(uri)
                ?: throw PlatformFailure("document_open_failed", "Document could not be opened")
            input.use { source ->
                FileOutputStream(temporary).use { output ->
                    val buffer = ByteArray(COPY_BUFFER_BYTES)
                    while (true) {
                        val count = source.read(buffer)
                        if (count < 0) break
                        if (count == 0) continue
                        total += count
                        if (total > MAX_STAGED_DOCUMENT_BYTES) {
                            throw PlatformFailure("document_too_large", "Document exceeds the staging limit")
                        }
                        digest.update(buffer, 0, count)
                        output.write(buffer, 0, count)
                    }
                    output.fd.sync()
                }
            }
            try {
                Files.move(
                    temporary.toPath(),
                    destination.toPath(),
                    StandardCopyOption.ATOMIC_MOVE,
                )
            } catch (_: AtomicMoveNotSupportedException) {
                Files.move(
                    temporary.toPath(),
                    destination.toPath(),
                    StandardCopyOption.REPLACE_EXISTING,
                )
            }
        } catch (failure: Exception) {
            temporary.delete()
            if (failure is PlatformFailure) throw failure
            throw PlatformFailure("document_stage_failed", failure.javaClass.simpleName)
        }

        return JSONObject()
            .put("staged_path", destination.absolutePath)
            .put("size_bytes", total)
            .put("sha256", digest.digest().toHex())
            .put("label", request.optString("label"))
            .put("mime_type", request.optString("mime_type"))
    }

    private fun deleteDocument(request: JSONObject): JSONObject {
        val root = documentRoot.canonicalFile
        val target = File(request.getString("staged_path")).canonicalFile
        ensureInsideDocumentRoot(target, root)
        if (target.exists() && !target.delete()) {
            throw PlatformFailure("document_delete_failed", "Staged document could not be deleted")
        }
        return JSONObject().put("deleted", true)
    }

    private fun wrapSecret(request: JSONObject): JSONObject {
        val alias = validateKeyAlias(request.getString("alias"))
        val plaintext = decodeBase64(request.getString("plaintext_base64"), "invalid_plaintext")
        try {
            val cipher = Cipher.getInstance(KEY_TRANSFORMATION)
            cipher.init(Cipher.ENCRYPT_MODE, getOrCreateSecretKey(alias))
            val ciphertext = cipher.doFinal(plaintext)
            val iv = cipher.iv
            if (iv.isEmpty() || iv.size > 255) {
                throw PlatformFailure("keystore_invalid_iv", "Keystore returned an invalid IV")
            }
            val envelope = ByteBuffer.allocate(2 + iv.size + ciphertext.size)
                .put(KEY_ENVELOPE_VERSION)
                .put(iv.size.toByte())
                .put(iv)
                .put(ciphertext)
                .array()
            return JSONObject().put(
                "wrapped_base64",
                Base64.encodeToString(envelope, Base64.NO_WRAP),
            )
        } finally {
            plaintext.fill(0)
        }
    }

    private fun unwrapSecret(request: JSONObject): JSONObject {
        val alias = validateKeyAlias(request.getString("alias"))
        val envelope = decodeBase64(request.getString("wrapped_base64"), "invalid_wrapper")
        if (envelope.size < 3 || envelope[0] != KEY_ENVELOPE_VERSION) {
            envelope.fill(0)
            throw PlatformFailure("invalid_wrapper", "Unsupported key wrapper envelope")
        }
        val ivSize = envelope[1].toInt() and 0xff
        if (ivSize !in 12..32 || envelope.size <= 2 + ivSize) {
            envelope.fill(0)
            throw PlatformFailure("invalid_wrapper", "Malformed key wrapper envelope")
        }
        val iv = envelope.copyOfRange(2, 2 + ivSize)
        val ciphertext = envelope.copyOfRange(2 + ivSize, envelope.size)
        envelope.fill(0)
        val keyStore = androidKeyStore()
        val key = keyStore.getKey(alias, null) as? SecretKey
            ?: throw PlatformFailure("keystore_key_missing", "Keystore key is unavailable")
        val plaintext = try {
            Cipher.getInstance(KEY_TRANSFORMATION).run {
                init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(GCM_TAG_BITS, iv))
                doFinal(ciphertext)
            }
        } catch (failure: Exception) {
            throw PlatformFailure("keystore_unwrap_failed", failure.javaClass.simpleName)
        } finally {
            iv.fill(0)
            ciphertext.fill(0)
        }
        return try {
            JSONObject().put(
                "plaintext_base64",
                Base64.encodeToString(plaintext, Base64.NO_WRAP),
            )
        } finally {
            plaintext.fill(0)
        }
    }

    private fun thermalStatus(): JSONObject {
        val powerManager = appContext.getSystemService(PowerManager::class.java)
        val available = Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q && powerManager != null
        val status = if (available) powerManager.currentThermalStatus else -1
        return JSONObject()
            .put("available", available)
            .put("status", status)
    }

    private fun networkStatus(): JSONObject {
        val manager = appContext.getSystemService(ConnectivityManager::class.java)
            ?: return JSONObject().put("connected", false).put("available", false)
        val network = manager.activeNetwork
        val capabilities = network?.let(manager::getNetworkCapabilities)
        val transports = JSONArray()
        if (capabilities != null) {
            if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI)) transports.put("wifi")
            if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR)) transports.put("cellular")
            if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)) transports.put("ethernet")
            if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_VPN)) transports.put("vpn")
            if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_BLUETOOTH)) transports.put("bluetooth")
        }
        return JSONObject()
            .put("available", true)
            .put("connected", capabilities?.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) == true)
            .put("validated", capabilities?.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED) == true)
            .put("metered", manager.isActiveNetworkMetered)
            .put("transports", transports)
    }

    private fun diagnosticsSnapshot(): JSONObject {
        val activityManager = appContext.getSystemService(ActivityManager::class.java)
        return JSONObject()
            .put("api_level", Build.VERSION.SDK_INT)
            .put("supported_abis", JSONArray(Build.SUPPORTED_ABIS.toList()))
            .put("low_ram_device", activityManager?.isLowRamDevice ?: false)
            .put("available_processors", Runtime.getRuntime().availableProcessors())
    }

    private fun getOrCreateSecretKey(alias: String): SecretKey {
        val keyStore = androidKeyStore()
        (keyStore.getKey(alias, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(
            KeyProperties.KEY_ALGORITHM_AES,
            ANDROID_KEYSTORE,
        )
        generator.init(
            KeyGenParameterSpec.Builder(
                alias,
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

    private fun validateKeyAlias(alias: String): String {
        if (!alias.startsWith("mukei.") || !KEY_ALIAS_PATTERN.matches(alias)) {
            throw PlatformFailure("invalid_keystore_alias", "Keystore alias is invalid")
        }
        return alias
    }

    private fun decodeBase64(value: String, code: String): ByteArray = try {
        Base64.decode(value, Base64.NO_WRAP)
    } catch (_: IllegalArgumentException) {
        throw PlatformFailure(code, "Base64 payload is invalid")
    }

    private fun ensureInsideDocumentRoot(target: File, root: File) {
        if (!target.toPath().startsWith(root.toPath()) || target == root) {
            throw PlatformFailure("document_path_rejected", "Document path is outside app storage")
        }
    }

    private fun successResponse(requestId: String, payload: JSONObject): JSONObject = JSONObject()
        .put("request_id", requestId)
        .put("status", "succeeded")
        .put("payload", payload)

    private fun failureResponse(
        requestId: String,
        code: String,
        safeMessage: String,
    ): JSONObject = JSONObject()
        .put("request_id", requestId)
        .put("status", "failed")
        .put("payload", JSONObject())
        .put("error_code", code)
        .put("error_message", safeMessage.take(MAX_ERROR_MESSAGE_CHARS))

    private class PlatformFailure(
        val code: String,
        val safeMessage: String,
    ) : Exception(safeMessage)

    private fun ByteArray.toHex(): String = joinToString(separator = "") { byte ->
        "%02x".format(byte.toInt() and 0xff)
    }

    companion object {
        private const val COPY_BUFFER_BYTES = 64 * 1024
        private const val MAX_STAGED_DOCUMENT_BYTES = 256L * 1024L * 1024L
        private const val MAX_ERROR_MESSAGE_CHARS = 160
        private const val ANDROID_KEYSTORE = "AndroidKeyStore"
        private const val KEY_TRANSFORMATION = "AES/GCM/NoPadding"
        private const val GCM_TAG_BITS = 128
        private const val KEY_ENVELOPE_VERSION: Byte = 1
        private val KEY_ALIAS_PATTERN = Regex("[A-Za-z0-9._-]{1,128}")
    }
}
