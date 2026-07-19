package ai.mukei.android

/**
 * Compatibility bridge for the storage branch's legacy Ready(securitySummary) runtime state.
 *
 * The Kotlin UI/UX files remain byte-for-byte identical to the Kotlin branch while this
 * branch keeps its storage/runtime implementation. Remove this adapter when the typed
 * readiness contract is ported natively.
 */
val BackendRuntimeHost.State.Ready.readiness: AppReadiness
    get() {
        val parts = securitySummary.split(" · ")
        fun part(index: Int): String = parts.getOrNull(index)?.trim().orEmpty()

        val database = when (part(0)) {
            "encrypted" -> ReadinessDimension(ReadinessStatus.READY, "encrypted")
            "not_configured", "unavailable" ->
                ReadinessDimension(ReadinessStatus.UNAVAILABLE, part(0))
            else -> ReadinessDimension(ReadinessStatus.UNKNOWN, "sqlcipher_unknown")
        }

        val projections = when (part(1)) {
            "encrypted" -> ReadinessDimension(ReadinessStatus.READY, "encrypted")
            "not_configured", "unavailable" ->
                ReadinessDimension(ReadinessStatus.UNAVAILABLE, "projections_unavailable")
            else -> ReadinessDimension(ReadinessStatus.UNKNOWN, "projections_unknown")
        }

        val inference = when (part(3)) {
            "ready" -> ReadinessDimension(ReadinessStatus.READY, "ready")
            "artifacts_required" ->
                ReadinessDimension(ReadinessStatus.ACTION_REQUIRED, "artifacts_required")
            "unavailable" -> ReadinessDimension(ReadinessStatus.UNAVAILABLE, "unavailable")
            else -> ReadinessDimension(ReadinessStatus.UNKNOWN, "inference_unknown")
        }

        val panic = when (part(4)) {
            "panic-contained" -> ReadinessDimension(ReadinessStatus.READY, "panic_contained")
            else -> ReadinessDimension(ReadinessStatus.DEGRADED, "panic_hook_missing")
        }

        return AppReadiness(
            secureRuntime = ReadinessDimension(ReadinessStatus.READY, "legacy_ready"),
            encryptedDatabase = database,
            encryptedProjections = projections,
            inference = inference,
            panicContainment = panic,
        )
    }
