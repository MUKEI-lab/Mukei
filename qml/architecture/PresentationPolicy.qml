pragma Singleton
import QtQuick

QtObject {
    function routeForLifecycle(state) {
        switch (state) {
        case "uninitialized":
        case "bootstrapping":
        case "booting":
        case "loading_config":
        case "opening_database":
        case "applying_migrations":
        case "hydrating_saf":
        case "reconciling_vector_store":
        case "loading_model":
            return "boot"
        case "needs_config":
            return "welcome"
        case "needs_database_key":
            return "unlock"
        case "ready":
        case "degraded":
            return "chat"
        case "audit_quarantined":
        case "quarantined":
        case "fatal_error":
            return "security"
        case "safe_mode":
            return "security"
        case "incompatible_contract":
            return "compatibility"
        default:
            return "boot"
        }
    }

    function presentationForSeverity(severity) {
        switch (severity) {
        case "fatal":
        case "security_critical":
            return "blocking"
        case "error":
            return "banner"
        case "warning":
            return "snackbar"
        default:
            return "snackbar"
        }
    }
}
