import QtQuick
import QtTest
import "../stores"

TestCase {
    name: "StartupLifecycleContracts"

    function init() {
        NavigationStore.currentRoute = "boot"
        NavigationStore.currentParameters = ({})
        NavigationStore.history = []
        NavigationStore.lifecycleLocked = true
        LifecycleStore.setLocalState("uninitialized", "")
    }

    function applyStage(stage) {
        LifecycleStore.applyEvent({
            category: "app_lifecycle",
            state: stage
        })
        NavigationStore.syncWithLifecycle(stage)
    }

    function test_progress_sequence_is_observable() {
        var stages = [
            "bootstrapping",
            "booting",
            "loading_config",
            "needs_database_key",
            "creating_wrapping_key",
            "creating_database_key",
            "wrapping_database_key",
            "opening_database"
        ]

        for (var i = 0; i < stages.length; ++i) {
            applyStage(stages[i])
            compare(LifecycleStore.state, stages[i])
            verify(LifecycleStore.title.length > 0)
            verify(LifecycleStore.description.length > 0)
            verify(LifecycleStore.title !== "Starting Mukei" || stages[i] === "bootstrapping")
        }
    }

    function test_ready_unlocks_chat_navigation() {
        applyStage("ready")
        compare(LifecycleStore.ready, true)
        compare(NavigationStore.lifecycleLocked, false)
        compare(NavigationStore.currentRoute, "chat")
    }

    function test_degraded_unlocks_chat_navigation() {
        applyStage("degraded")
        compare(LifecycleStore.ready, true)
        compare(LifecycleStore.degraded, true)
        compare(NavigationStore.lifecycleLocked, false)
        compare(NavigationStore.currentRoute, "chat")
    }

    function test_terminal_failure_keeps_diagnostics_available() {
        applyStage("fatal_error")
        compare(NavigationStore.lifecycleLocked, true)
        compare(NavigationStore.currentRoute, "security")
        verify(NavigationStore.navigate("diagnostics", ({ source: "test" }), false))
        compare(NavigationStore.currentRoute, "diagnostics")
    }
}
