import QtQuick
import QtTest
import "../screens"
import "../stores"

TestCase {
    id: testCase
    name: "InteractionContracts"
    when: windowShown
    width: 480
    height: 800

    Component { id: safeModeComponent; SafeModeScreen {} }
    Component { id: diagnosticsComponent; DiagnosticsScreen {} }

    function resetState() {
        ErrorStore.dismiss()
        LifecycleStore.setLocalState("fatal_error", "")
        NavigationStore.currentRoute = "security"
        NavigationStore.currentParameters = ({})
        NavigationStore.history = []
        NavigationStore.lifecycleLocked = true
    }

    function init() {
        resetState()
    }

    function cleanup() {
        ErrorStore.dismiss()
        NavigationStore.history = []
    }

    function test_safe_mode_continue_opens_limited_chat() {
        var page = createTemporaryObject(safeModeComponent, testCase)
        verify(page !== null)
        var button = findChild(page, "safeModeContinueButton")
        verify(button !== null, "Continue button must be discoverable")

        mouseClick(button)

        tryCompare(LifecycleStore, "state", "degraded")
        compare(NavigationStore.lifecycleLocked, false)
        compare(NavigationStore.currentRoute, "chat")
        verify(LifecycleStore.safeDetail.indexOf("limited mode") >= 0)
    }

    function test_safe_mode_diagnostics_route_is_available_while_locked() {
        var page = createTemporaryObject(safeModeComponent, testCase)
        verify(page !== null)
        var button = findChild(page, "safeModeDiagnosticsButton")
        verify(button !== null, "Diagnostics button must be discoverable")

        mouseClick(button)

        compare(NavigationStore.lifecycleLocked, true)
        compare(NavigationStore.currentRoute, "diagnostics")
        compare(NavigationStore.currentParameters.from, "safe_mode")
    }

    function test_safe_mode_reset_never_silently_ignores_the_tap() {
        var page = createTemporaryObject(safeModeComponent, testCase)
        verify(page !== null)
        var button = findChild(page, "safeModeResetButton")
        verify(button !== null, "Reset button must be discoverable")

        mouseClick(button)

        verify(ErrorStore.hasError)
        compare(ErrorStore.currentError.code, "ERR_RESET_REQUIRES_REINSTALL")
        verify(ErrorStore.currentError.safeMessage.length > 0)
    }

    function test_diagnostics_controls_are_discoverable() {
        NavigationStore.currentRoute = "diagnostics"
        var page = createTemporaryObject(diagnosticsComponent, testCase)
        verify(page !== null)

        verify(findChild(page, "diagnosticsRefreshButton") !== null)
        verify(findChild(page, "diagnosticsExportButton") !== null)
        verify(findChild(page, "diagnosticsBackButton") !== null)
    }
}
