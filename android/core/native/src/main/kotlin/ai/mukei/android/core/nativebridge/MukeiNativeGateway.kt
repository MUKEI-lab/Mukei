package ai.mukei.android.core.nativebridge

import java.io.Closeable
import java.security.SecureRandom
import java.util.concurrent.atomic.AtomicBoolean

interface MukeiNativeGateway : Closeable {
    fun protocolCapabilities(): ByteArray
    fun securityStatus(): ByteArray
    fun submitCommand(commandJson: ByteArray): ByteArray

    fun drainEvents(
        maximumEvents: Int = 32,
        timeoutMilliseconds: Long = 1_000,
    ): ByteArray

    fun drainPlatformRequests(
        maximumRequests: Int = 8,
        timeoutMilliseconds: Long = 0,
    ): ByteArray

    fun submitPlatformResponse(responseJson: ByteArray): ByteArray
    fun requestSnapshot(domain: String): ByteArray
    fun shutdown(): ByteArray
}

class RustNativeGateway private constructor(
    private val nativeHandle: Long,
    private val secureRuntime: Boolean,
) : MukeiNativeGateway {
    private val closed = AtomicBoolean(false)

    override fun protocolCapabilities(): ByteArray {
        checkOpen()
        return NativeBindings.protocolCapabilities(nativeHandle)
    }

    override fun securityStatus(): ByteArray {
        checkOpen()
        return NativeBindings.securityStatus(nativeHandle)
    }

    fun configureRemoteTools(
        braveKey: ByteArray,
        tavilyKey: ByteArray,
    ): ByteArray {
        checkOpen()
        require(braveKey.isNotEmpty() && braveKey.size <= MAX_PROVIDER_KEY_BYTES)
        require(tavilyKey.isNotEmpty() && tavilyKey.size <= MAX_PROVIDER_KEY_BYTES)
        return NativeBindings.configureRemoteTools(nativeHandle, braveKey, tavilyKey)
    }

    override fun submitCommand(commandJson: ByteArray): ByteArray {
        checkOpen()
        require(commandJson.isNotEmpty()) { "Command envelope must not be empty" }
        return NativeBindings.submitCommand(nativeHandle, commandJson)
    }

    override fun drainEvents(
        maximumEvents: Int,
        timeoutMilliseconds: Long,
    ): ByteArray {
        checkOpen()
        require(maximumEvents in 1..256) { "maximumEvents must be between 1 and 256" }
        require(timeoutMilliseconds in 0..30_000) {
            "timeoutMilliseconds must be between 0 and 30000"
        }
        return NativeBindings.drainEvents(nativeHandle, maximumEvents, timeoutMilliseconds)
    }

    override fun drainPlatformRequests(
        maximumRequests: Int,
        timeoutMilliseconds: Long,
    ): ByteArray {
        checkOpen()
        require(maximumRequests in 1..32) { "maximumRequests must be between 1 and 32" }
        require(timeoutMilliseconds in 0..30_000) {
            "timeoutMilliseconds must be between 0 and 30000"
        }
        return NativeBindings.drainPlatformRequests(
            nativeHandle,
            maximumRequests,
            timeoutMilliseconds,
        )
    }

    override fun submitPlatformResponse(responseJson: ByteArray): ByteArray {
        checkOpen()
        require(responseJson.isNotEmpty()) { "Platform response must not be empty" }
        require(responseJson.size <= 512 * 1024) { "Platform response is too large" }
        return NativeBindings.submitPlatformResponse(nativeHandle, responseJson)
    }

    override fun requestSnapshot(domain: String): ByteArray {
        checkOpen()
        require(domain.isNotBlank()) { "Snapshot domain must not be blank" }
        return NativeBindings.requestSnapshot(nativeHandle, domain)
    }

    override fun shutdown(): ByteArray {
        checkOpen()
        return NativeBindings.shutdownRuntime(nativeHandle)
    }

    override fun close() {
        if (closed.compareAndSet(false, true)) {
            try {
                NativeBindings.shutdownRuntime(nativeHandle)
            } finally {
                if (secureRuntime) {
                    NativeBindings.destroySecureRuntime(nativeHandle)
                } else {
                    NativeBindings.destroyRuntime(nativeHandle)
                }
            }
        }
    }

    private fun checkOpen() {
        check(!closed.get()) { "Native runtime is already closed" }
    }

    companion object {
        private const val MAX_PROVIDER_KEY_BYTES = 16 * 1024

        fun create(configJson: ByteArray): RustNativeGateway {
            require(configJson.isNotEmpty()) { "Runtime configuration must not be empty" }
            val handle = NativeBindings.createRuntime(configJson)
            check(handle > 0L) { "Native runtime creation failed" }
            return RustNativeGateway(handle, secureRuntime = false)
        }

        fun createSecure(
            configJson: ByteArray,
            databaseKey: ByteArray,
        ): RustNativeGateway {
            require(configJson.isNotEmpty()) { "Runtime configuration must not be empty" }
            require(databaseKey.size == 32) { "SQLCipher key must be exactly 32 bytes" }
            val handle = NativeBindings.createSecureRuntime(configJson, databaseKey)
            check(handle > 0L) { "Secure native runtime creation failed" }
            return RustNativeGateway(handle, secureRuntime = true)
        }
    }
}

internal object NativeBindings {
    private val secureRandom = SecureRandom()

    init {
        System.loadLibrary("mukei_android")
    }

    /**
     * Generate SQLCipher key material on the Android/JCA side.
     *
     * Key generation must not depend on JNI availability because this is part of
     * the secure runtime bootstrap that precedes native runtime creation.
     */
    fun generateDatabaseKey(): ByteArray = ByteArray(DATABASE_KEY_BYTES).also(secureRandom::nextBytes)

    external fun createRuntime(configJson: ByteArray): Long

    external fun createSecureRuntime(
        configJson: ByteArray,
        databaseKey: ByteArray,
    ): Long

    external fun configureRemoteTools(
        handle: Long,
        braveKey: ByteArray,
        tavilyKey: ByteArray,
    ): ByteArray

    external fun shutdownRuntime(handle: Long): ByteArray
    external fun destroyRuntime(handle: Long)
    external fun destroySecureRuntime(handle: Long)
    external fun protocolCapabilities(handle: Long): ByteArray
    external fun securityStatus(handle: Long): ByteArray

    external fun submitCommand(
        handle: Long,
        commandJson: ByteArray,
    ): ByteArray

    external fun drainEvents(
        handle: Long,
        maximumEvents: Int,
        timeoutMilliseconds: Long,
    ): ByteArray

    external fun drainPlatformRequests(
        handle: Long,
        maximumRequests: Int,
        timeoutMilliseconds: Long,
    ): ByteArray

    external fun submitPlatformResponse(
        handle: Long,
        responseJson: ByteArray,
    ): ByteArray

    external fun requestSnapshot(
        handle: Long,
        domain: String,
    ): ByteArray

    private const val DATABASE_KEY_BYTES = 32
}
