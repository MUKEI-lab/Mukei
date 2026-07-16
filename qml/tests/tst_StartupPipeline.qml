import QtQuick
import QtTest
import "../architecture"
import "../events"
import "../stores"

TestCase {
    id: testCase
    name: "StartupPipeline"
    when: windowShown

    Component { id: agentComponent; FakeProtocolAgent {} }
    Component {
        id: runtimeComponent
        QtObject {
            property string configPath: "/tmp/mukei-test-config.toml"
            property bool autoInitialize: true
        }
    }

    property var agent: null
    property var runtime: null

    function resetArchitecture() {
        EventDispatcher.reset()
        ContractStore.reset()
        ErrorStore.dismiss()
        LifecycleStore.setLocalState("uninitialized", "")
        NavigationStore.currentRoute = "boot"
        NavigationStore.currentParameters = ({})
        NavigationStore.history = []
        NavigationStore.lifecycleLocked = true
        AppCoordinator.configured = false
        AppCoordinator.started = false
        AppCoordinator.readyHydrated = false
        AppCoordinator.readyHydrationPending = false
        AppCoordinator.runtimeSource = null
    }

    function init() {
        resetArchitecture()
        agent = createTemporaryObject(agentComponent, testCase)
        runtime = createTemporaryObject(runtimeComponent, testCase)
        verify(agent !== null)
        verify(runtime !== null)
    }

    function cleanup() {
        LifecycleStore.setLocalState("ready", "")
        AppCoordinator.started = false
        AppCoordinator.configured = false
        agent = null
        runtime = null
    }

    function startWithSignalMode(mode) {
        agent.signalMode = mode
        AppCoordinator.configure(agent, null, runtime)
        AppCoordinator.start()
    }

    function test_snake_case_bridge_signal_reaches_ready() {
        startWithSignalMode("snake")

        tryCompare(LifecycleStore, "state", "ready", 2000)
        compare(agent.submittedCommands.length, 1)
        compare(agent.submittedCommands[0].command_type, "app.initialize")
        compare(NavigationStore.lifecycleLocked, false)
        tryCompare(NavigationStore, "currentRoute", "chat", 2000)
    }

    function test_camel_case_bridge_signal_reaches_ready() {
        startWithSignalMode("camel")

        tryCompare(LifecycleStore, "state", "ready", 2000)
        compare(agent.submittedCommands.length, 1)
        compare(NavigationStore.lifecycleLocked, false)
        tryCompare(NavigationStore, "currentRoute", "chat", 2000)
    }

    function test_acknowledged_without_events_is_explicitly_observable() {
        agent.emitLifecycleEvents = false
        startWithSignalMode("snake")

        compare(agent.submittedCommands.length, 1)
        compare(LifecycleStore.state, "initialize_acknowledged")
        verify(LifecycleStore.safeDetail.indexOf("accepted") >= 0)
        compare(NavigationStore.currentRoute, "boot")
        compare(NavigationStore.lifecycleLocked, true)
    }
}
