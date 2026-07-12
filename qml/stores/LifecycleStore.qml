pragma Singleton
import QtQuick

QtObject {
    property string state: "uninitialized"
    property string previousState: ""
    property string androidStorageState: "unknown"
    property string safeDetail: ""
    readonly property bool ready: state === "ready" || state === "degraded"
    readonly property bool degraded: state === "degraded"
    readonly property bool incompatible: state === "incompatible_contract"
    readonly property bool quarantined: state === "quarantined"
                                          || state === "audit_quarantined"
                                          || state === "fatal_error"
                                          || incompatible
    readonly property bool interactive: ready && !quarantined
    readonly property bool busy: !interactive && !quarantined
    readonly property string title: titleForState(state)
    readonly property string description: descriptionForState(state)

    signal changed(string state)

    function setLocalState(nextState, detail) {
        if (state === nextState && safeDetail === (detail || ""))
            return
        previousState = state
        state = nextState
        safeDetail = detail || ""
        changed(state)
    }

    function applyEvent(event) {
        if (!event || event.category !== "app_lifecycle")
            return
        if (event.android_storage && event.android_storage.state)
            androidStorageState = event.android_storage.state
        setLocalState(event.state, "")
    }

    function titleForState(value) {
        switch (value) {
        case "needs_database_key": return qsTr("Preparing secure storage")
        case "creating_wrapping_key": return qsTr("Creating device protection")
        case "creating_database_key": return qsTr("Creating private storage key")
        case "wrapping_database_key": return qsTr("Protecting private storage key")
        case "unwrapping_database_key": return qsTr("Unlocking private storage")
        case "key_invalidated": return qsTr("Device security key changed")
        case "wrapped_key_corrupt": return qsTr("Secure key data is damaged")
        case "database_open_failed": return qsTr("Private storage could not open")
        case "reset_required": return qsTr("Secure storage reset required")
        case "needs_config": return qsTr("Configuration required")
        case "opening_database": return qsTr("Opening private storage")
        case "applying_migrations": return qsTr("Updating local data")
        case "hydrating_saf": return qsTr("Restoring document access")
        case "reconciling_vector_store": return qsTr("Checking local knowledge")
        case "loading_model": return qsTr("Preparing your model")
        case "degraded": return qsTr("Mukei is ready with limited capability")
        case "quarantined":
        case "audit_quarantined": return qsTr("Mukei is in protected mode")
        case "fatal_error": return qsTr("Mukei could not start safely")
        case "incompatible_contract": return qsTr("Mukei components are incompatible")
        case "ready": return qsTr("Mukei is ready")
        default: return qsTr("Starting Mukei")
        }
    }

    function descriptionForState(value) {
        if (safeDetail.length > 0)
            return safeDetail
        switch (value) {
        case "needs_database_key": return qsTr("Waiting for the native secure-key provider. No private data is opened yet.")
        case "creating_wrapping_key": return qsTr("Mukei is establishing device-bound protection for the encrypted database key.")
        case "creating_database_key": return qsTr("A new random database key is being created in memory for first-time setup.")
        case "wrapping_database_key": return qsTr("The database key is being protected before any wrapped material is persisted.")
        case "unwrapping_database_key": return qsTr("The existing database key is being recovered through the platform security provider.")
        case "key_invalidated": return qsTr("The platform security key is no longer usable. Recovery or a secure reset is required.")
        case "wrapped_key_corrupt": return qsTr("Stored wrapped key material is malformed and cannot be used safely.")
        case "database_open_failed": return qsTr("The encrypted database could not be opened. The failure was classified instead of retrying indefinitely.")
        case "reset_required": return qsTr("Mukei will not overwrite existing secure material automatically. Use the supported recovery or reset flow.")
        case "needs_config": return qsTr("A valid app-private configuration is needed before startup can continue.")
        case "opening_database": return qsTr("Encrypted local storage is being opened.")
        case "applying_migrations": return qsTr("Your local data is being upgraded safely.")
        case "hydrating_saf": return qsTr("Previously approved document access is being restored.")
        case "reconciling_vector_store": return qsTr("Local document indexes are being reconciled.")
        case "loading_model": return qsTr("The selected on-device model is being prepared.")
        case "degraded": return qsTr("Some actions are unavailable until the reported condition is resolved.")
        case "quarantined":
        case "audit_quarantined": return qsTr("Write operations are disabled to protect your local data.")
        case "fatal_error": return qsTr("Review the safe error details or restart after resolving the problem.")
        case "incompatible_contract": return qsTr("Update the frontend and local service together before private data is opened.")
        default: return qsTr("This happens locally on your device.")
        }
    }
}
