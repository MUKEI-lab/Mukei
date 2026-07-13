pragma Singleton
import QtQuick

QtObject {
    property var currentError: null
    property int revision: 0
    readonly property bool hasError: currentError !== null
    readonly property string presentation: hasError
            ? (typeof PresentationPolicy !== "undefined"
               ? PresentationPolicy.presentationForSeverity(currentError.severity || "error")
               : fallbackPresentationForSeverity(currentError.severity || "error"))
            : "none"

    signal errorPresented(var error)
    signal errorDismissed

    function fallbackPresentationForSeverity(severity) {
        switch (severity) {
        case "fatal":
        case "security_critical":
            return "blocking"
        case "error":
            return "banner"
        case "warning":
        default:
            return "snackbar"
        }
    }

    function normalize(error, fallbackCode) {
        var source = error && typeof error === "object" ? error : ({})
        return {
            code: typeof source.code === "string" ? source.code : (fallbackCode || "ERR_UI_UNKNOWN"),
            severity: typeof source.severity === "string" ? source.severity : "error",
            recoverable: source.recoverable === true,
            safeMessage: source.user_message || source.safe_message || source.message || qsTr("Mukei could not complete that action."),
            suggestedAction: source.suggested_action || "",
            operationId: source.operation_id || "",
            feature: source.feature || ""
        }
    }

    function push(error, fallbackCode) {
        currentError = normalize(error, fallbackCode)
        revision += 1
        errorPresented(currentError)
    }

    function applyEvent(event) {
        if (!event)
            return
        if (event.category === "error" || event.category === "chat_failed" || event.category === "download_failed"
                || (event.category === "operation_lifecycle" && event.state === "failed"))
            push(event.error, "ERR_UI_EVENT")
    }

    function dismiss() {
        currentError = null
        revision += 1
        errorDismissed()
    }
}
