package ai.mukei.android.protocol

import java.nio.charset.StandardCharsets
import org.json.JSONObject

/** Decoder for the runtime capability contract returned by the native transport. */
object RuntimeContractJsonCodec {
    fun decode(bytes: ByteArray): RuntimeContractSnapshot {
        val root = try {
            JSONObject(String(bytes, StandardCharsets.UTF_8))
        } catch (_: Throwable) {
            throw ProtocolCodecException("invalid_runtime_contract_json")
        }
        root.optJSONObject("error")?.let { error ->
            throw ProtocolCodecException(error.optString("code", "runtime_contract_failed"))
        }

        val protocolObject = root.optJSONObject("protocol")
            ?: throw ProtocolCodecException("missing_runtime_protocol")
        val currentVersionObject = protocolObject.optJSONObject("current_version")
            ?: throw ProtocolCodecException("missing_runtime_protocol_version")
        val currentVersion = ProtocolVersion(
            major = currentVersionObject.optInt("major", -1),
            minor = currentVersionObject.optInt("minor", -1),
        )
        if (!currentVersion.isCompatible()) {
            throw ProtocolCodecException("unsupported_protocol")
        }

        val capabilitiesJson = protocolObject.optJSONArray("capabilities")
            ?: throw ProtocolCodecException("missing_runtime_capabilities")
        val capabilities = buildList(capabilitiesJson.length()) {
            for (index in 0 until capabilitiesJson.length()) {
                val value = capabilitiesJson.optString(index).trim()
                if (value.isEmpty()) throw ProtocolCodecException("invalid_runtime_capability")
                add(value)
            }
        }

        val clientKind = when (root.optString("client_kind")) {
            "android" -> ClientKind.ANDROID
            else -> throw ProtocolCodecException("unsupported_client_kind")
        }
        val runtimeSessionId = root.optString("runtime_session_id").trim()
        if (!isProtocolId(runtimeSessionId)) {
            throw ProtocolCodecException("invalid_runtime_session_id")
        }
        val minimumSupportedPeerMajor = protocolObject.optInt("minimum_supported_peer_major", -1)
        if (minimumSupportedPeerMajor <= 0) {
            throw ProtocolCodecException("invalid_minimum_peer_protocol")
        }
        val snapshotSchemaVersion = root.optInt("snapshot_schema_version", -1)
        if (snapshotSchemaVersion <= 0) {
            throw ProtocolCodecException("invalid_snapshot_schema_version")
        }

        return RuntimeContractSnapshot(
            clientKind = clientKind,
            runtimeSessionId = runtimeSessionId,
            protocol = ProtocolCapabilitySnapshot(
                currentVersion = currentVersion,
                minimumSupportedPeerMajor = minimumSupportedPeerMajor,
                capabilities = capabilities,
            ),
            snapshotSchemaVersion = snapshotSchemaVersion,
        )
    }

    private fun isProtocolId(value: String): Boolean =
        value.isNotEmpty() &&
            value.length <= ProtocolLimits.MAX_PROTOCOL_ID_LENGTH &&
            value == value.trim() &&
            value.all { character ->
                character.isLetterOrDigit() || character == '-' || character == '_' ||
                    character == '.' || character == ':' || character == '/'
            }
}
