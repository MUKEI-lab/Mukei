package ai.mukei.android.core.nativebridge

import java.io.Closeable
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Narrow Kotlin boundary for the Rust runtime.
 *
 * Feature modules depend on this interface and never call [NativeBindings]
 * directly. All returned payloads are bounded Protocol V2 UTF-8 JSON bytes.
 */
interface MukeiNativeGateway : Closeable {
    /** Returns the negotiated native runtime and transport contract. */
    fun protocolCapabilities(): ByteArray

    /** Submits one Protocol V2 command envelope. */
    fun submitCommand(commandJson: ByteArray): ByteArray

    /** Drains one bounded Protocol V2 event batch. */
    fun drainEvents(
        maximumEvents: Int = 32,
        timeoutMilliseconds: Long = 1_000,
    ): ByteArray

    /** Requests one authoritative Protocol V2 snapshot. */
    fun requestSnapshot(domain: String): ByteArray
}

class RustNativeGateway private constructor(
    private val nativeHandle: Long,
) : MukeiNativeGateway {
    private val closed = AtomicBoolean(false)

    override fun protocolCapabilities(): ByteArray {
        checkOpen()
        return NativeBindings.protocolCapabilities(nativeHandle)
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
        return NativeBindings.drainEvents(
            nativeHandle,
            maximumEvents,
            timeoutMilliseconds,
        )
    }

    override fun requestSnapshot(domain: String): ByteArray {
        checkOpen()
        require(domain.isNotBlank()) { "Snapshot domain must not be blank" }
        return NativeBindings.requestSnapshot(nativeHandle, domain)
    }

    override fun close() {
        if (closed.compareAndSet(false, true)) {
            NativeBindings.destroyRuntime(nativeHandle)
        }
    }

    private fun checkOpen() {
        check(!closed.get()) { "Native runtime is already closed" }
    }

    companion object {
        fun create(configJson: ByteArray): RustNativeGateway {
            require(configJson.isNotEmpty()) { "Runtime configuration must not be empty" }
            val handle = NativeBindings.createRuntime(configJson)
            check(handle > 0L) { "Native runtime creation failed" }
            return RustNativeGateway(handle)
        }
    }
}

internal object NativeBindings {
    init {
        System.loadLibrary("mukei_android")
    }

    external fun createRuntime(configJson: ByteArray): Long

    external fun destroyRuntime(handle: Long)

    external fun protocolCapabilities(handle: Long): ByteArray

    external fun submitCommand(
        handle: Long,
        commandJson: ByteArray,
    ): ByteArray

    external fun drainEvents(
        handle: Long,
        maximumEvents: Int,
        timeoutMilliseconds: Long,
    ): ByteArray

    external fun requestSnapshot(
        handle: Long,
        domain: String,
    ): ByteArray
}
