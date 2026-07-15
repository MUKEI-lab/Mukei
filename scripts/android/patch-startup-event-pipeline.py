#!/usr/bin/env python3
"""Repair the Android stub startup event pipeline for full-UI device validation.

The production bridge is not changed. This build-time patch makes the local C++
stub expose a conventional camelCase Qt signal, lets QML accept either signal
spelling, emits a terminal app.initialize operation event, and adds a stub-only
watchdog so a failed signal connection cannot leave the UI on the boot screen.
"""
from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MAIN = ROOT / "qml/main.cpp"
EVENT_DISPATCHER = ROOT / "qml/events/EventDispatcher.qml"
APP_COORDINATOR = ROOT / "qml/architecture/AppCoordinator.qml"


def patch_main() -> None:
    text = MAIN.read_text(encoding="utf-8")

    signal_anchor = "    void event_emitted(const QString &eventJson);\n    void async_result(const QString &resultJson);"
    if signal_anchor not in text:
        raise SystemExit("MukeiAgentStub event signal anchor not found")
    text = text.replace(
        signal_anchor,
        "    void event_emitted(const QString &eventJson);\n"
        "    void eventEmitted(const QString &eventJson);\n"
        "    void async_result(const QString &resultJson);",
        1,
    )

    emit_anchor = "        emit event_emitted(QString::fromUtf8(QJsonDocument(event).toJson(QJsonDocument::Compact)));"
    if emit_anchor not in text:
        raise SystemExit("MukeiAgentStub emitEvent anchor not found")
    text = text.replace(
        emit_anchor,
        "        const QString eventJson = QString::fromUtf8(\n"
        "            QJsonDocument(event).toJson(QJsonDocument::Compact));\n"
        "        qInfo().noquote() << \"MukeiStub eventEmitted\" << eventJson;\n"
        "        emit eventEmitted(eventJson);",
        1,
    )

    init_anchor = '''            if (commandType == QStringLiteral("app.initialize")) {
                m_appContext = context;
                initialize(payload.value(QStringLiteral("config_path")).toString());
'''
    if init_anchor not in text:
        raise SystemExit("stub app.initialize dispatch anchor not found")
    text = text.replace(
        init_anchor,
        '''            if (commandType == QStringLiteral("app.initialize")) {
                m_appContext = context;
                initialize(payload.value(QStringLiteral("config_path")).toString());
                emitOperationLifecycle(context, true, QJsonObject{
                    {QStringLiteral("initialized"), true},
                    {QStringLiteral("runtime"), QStringLiteral("stub")}
                });
''',
        1,
    )

    MAIN.write_text(text, encoding="utf-8")


def patch_event_dispatcher() -> None:
    text = EVENT_DISPATCHER.read_text(encoding="utf-8")

    agent_anchor = '''    Connections {
        target: root.agentSource === null ? null : root.agentSource
        function onEvent_emitted(eventJson) { root.ingest(eventJson, "agent") }
    }
'''
    if agent_anchor not in text:
        raise SystemExit("agent Connections anchor not found")
    text = text.replace(
        agent_anchor,
        '''    Connections {
        target: root.agentSource === null ? null : root.agentSource
        ignoreUnknownSignals: true
        function onEvent_emitted(eventJson) { root.ingest(eventJson, "agent") }
        function onEventEmitted(eventJson) { root.ingest(eventJson, "agent") }
    }
''',
        1,
    )

    bridge_anchor = '''    Connections {
        target: root.bridgeSource === null ? null : root.bridgeSource
        function onEvent_emitted(eventJson) { root.ingest(eventJson, "bridge") }
    }
'''
    if bridge_anchor not in text:
        raise SystemExit("bridge Connections anchor not found")
    text = text.replace(
        bridge_anchor,
        '''    Connections {
        target: root.bridgeSource === null ? null : root.bridgeSource
        ignoreUnknownSignals: true
        function onEvent_emitted(eventJson) { root.ingest(eventJson, "bridge") }
        function onEventEmitted(eventJson) { root.ingest(eventJson, "bridge") }
    }
''',
        1,
    )

    EVENT_DISPATCHER.write_text(text, encoding="utf-8")


def patch_app_coordinator() -> None:
    text = APP_COORDINATOR.read_text(encoding="utf-8")

    signals_anchor = '''    signal architectureReady
    signal readyStateHydrated
'''
    if signals_anchor not in text:
        raise SystemExit("AppCoordinator signal anchor not found")
    text = text.replace(
        signals_anchor,
        '''    signal architectureReady
    signal readyStateHydrated

    Timer {
        id: stubStartupWatchdog
        interval: 1500
        repeat: false
        onTriggered: {
            if (!runtimeSource || runtimeSource.realBridge !== false || LifecycleStore.ready)
                return
            console.warn("MukeiStartup: stub event pipeline watchdog applied a safe ready snapshot")
            AppCoordinator.applyEvent({
                schema_version: 1,
                category: "app_lifecycle",
                state: "ready",
                capabilities: {
                    can_initialize: false,
                    can_send_message: true,
                    can_stop_generation: false,
                    can_download_model: true,
                    can_stop_download: false,
                    can_switch_model: true,
                    can_delete_model: true,
                    can_clear_conversation: true,
                    can_open_settings: true,
                    needs_config: false,
                    needs_storage_permission: false,
                    active_model_ready: false,
                    is_busy: false,
                    is_downloading: false,
                    is_inferencing: false
                },
                android_storage: { state: "unknown" }
            })
            OperationStore.removeByType("app.initialize")
        }
    }
''',
        1,
    )

    dispatch_anchor = '''            IntentDispatcher.dispatch({
                type: "app.initialize",
                configPath: runtimeSource.configPath
            })
'''
    if dispatch_anchor not in text:
        raise SystemExit("AppCoordinator app.initialize anchor not found")
    text = text.replace(
        dispatch_anchor,
        '''            IntentDispatcher.dispatch({
                type: "app.initialize",
                configPath: runtimeSource.configPath
            })
            if (runtimeSource.realBridge === false)
                stubStartupWatchdog.restart()
''',
        1,
    )

    lifecycle_anchor = '''        if (event.category === "app_lifecycle") {
            NavigationStore.syncWithLifecycle(event.state)
'''
    if lifecycle_anchor not in text:
        raise SystemExit("AppCoordinator lifecycle event anchor not found")
    text = text.replace(
        lifecycle_anchor,
        '''        if (event.category === "app_lifecycle") {
            if (event.state === "ready" || event.state === "degraded")
                stubStartupWatchdog.stop()
            NavigationStore.syncWithLifecycle(event.state)
''',
        1,
    )

    APP_COORDINATOR.write_text(text, encoding="utf-8")


def main() -> int:
    patch_main()
    patch_event_dispatcher()
    patch_app_coordinator()
    print("Patched stub startup signal aliases, terminal initialization, and watchdog")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
