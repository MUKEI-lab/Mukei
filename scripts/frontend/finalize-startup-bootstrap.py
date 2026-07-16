#!/usr/bin/env python3
from pathlib import Path

from install_stabilization_batch import main as install_stabilization_batch

ROOT = Path(__file__).resolve().parents[2]
APP = ROOT / "qml/architecture/AppCoordinator.qml"
LIFE = ROOT / "qml/stores/LifecycleStore.qml"
RUST = ROOT / "rust/crates/mukei-bridge/src/lib.rs"
PROTOCOL = ROOT / "rust/crates/mukei-bridge/src/protocol.rs"


def replace_once(text, old, new, marker):
    if marker in text:
        return text
    if old not in text:
        raise SystemExit(f"missing patch anchor: {marker}")
    return text.replace(old, new, 1)


def patch_app(text):
    text = replace_once(text,
'''    signal architectureReady
    signal readyStateHydrated
''',
'''    signal architectureReady
    signal readyStateHydrated

    Timer {
        id: startupWatchdog
        interval: 45000
        repeat: false
        onTriggered: {
            if (LifecycleStore.ready || LifecycleStore.quarantined)
                return
            var detail = qsTr("Secure startup did not complete within 45 seconds. Restart Mukei; if it repeats, collect diagnostics.")
            LifecycleStore.setLocalState("fatal_error", detail)
            NavigationStore.syncWithLifecycle(LifecycleStore.state)
            ErrorStore.push({ code: "ERR_STARTUP_TIMEOUT", severity: "fatal",
                              recoverable: true, user_message: detail,
                              suggested_action: "restart" }, "ERR_STARTUP_TIMEOUT")
        }
    }
''', 'ERR_STARTUP_TIMEOUT')
    text = replace_once(text,
'''        LifecycleStore.setLocalState("bootstrapping", "")
        NavigationStore.syncWithLifecycle(LifecycleStore.state)
        if (runtimeSource && runtimeSource.autoInitialize === true) {
''',
'''        LifecycleStore.setLocalState("bootstrapping", "")
        NavigationStore.syncWithLifecycle(LifecycleStore.state)
        startupWatchdog.restart()
        if (runtimeSource && runtimeSource.autoInitialize === true) {
''', 'startupWatchdog.restart()')
    text = replace_once(text,
'''        LifecycleStore.setLocalState("bootstrapping", "")
        NavigationStore.syncWithLifecycle(LifecycleStore.state)
        startupWatchdog.restart()
        if (runtimeSource && runtimeSource.autoInitialize === true) {
            IntentDispatcher.dispatch({
                type: "app.initialize",
                configPath: runtimeSource.configPath
            })
        } else {
''',
'''        LifecycleStore.setLocalState("bootstrapping", "")
        NavigationStore.syncWithLifecycle(LifecycleStore.state)
        startupWatchdog.restart()
        if (runtimeSource && runtimeSource.autoInitialize === true) {
            LifecycleStore.setLocalState(
                        "initialize_submitted",
                        qsTr("The production frontend submitted the secure startup command."))
            var accepted = IntentDispatcher.dispatch({
                type: "app.initialize",
                configPath: runtimeSource.configPath
            })
            if (accepted && !LifecycleStore.ready && !LifecycleStore.quarantined) {
                LifecycleStore.setLocalState(
                            "initialize_acknowledged",
                            qsTr("The local runtime accepted the startup command and is scheduling native initialization."))
            } else if (!accepted) {
                startupWatchdog.stop()
                var detail = qsTr("The local runtime rejected the startup command before native initialization began.")
                LifecycleStore.setLocalState("fatal_error", detail)
                NavigationStore.syncWithLifecycle(LifecycleStore.state)
                ErrorStore.push({ code: "ERR_STARTUP_COMMAND_REJECTED", severity: "fatal",
                                  recoverable: true, user_message: detail,
                                  suggested_action: "collect_diagnostics" },
                                "ERR_STARTUP_COMMAND_REJECTED")
            }
        } else {
''', '"initialize_acknowledged"')
    text = replace_once(text,
'''        ErrorStore.applyEvent(event)

        if ((event.command_type === "recovery.resume" || event.command_type === "recovery.regenerate")
''',
'''        ErrorStore.applyEvent(event)

        if (event.category === "error" && !LifecycleStore.ready) {
            var source = event.error && event.error.source ? String(event.error.source) : ""
            if (["initialize", "secure_bootstrap", "database_open", "production_safety"].indexOf(source) >= 0) {
                var detail = event.error.user_message || event.error.safe_message
                        || qsTr("Secure startup could not complete safely.")
                startupWatchdog.stop()
                LifecycleStore.setLocalState("fatal_error", detail)
                NavigationStore.syncWithLifecycle(LifecycleStore.state)
            }
        }

        if ((event.command_type === "recovery.resume" || event.command_type === "recovery.regenerate")
''', 'Secure startup could not complete safely.')
    text = replace_once(text,
'''        if (event.category === "app_lifecycle") {
            NavigationStore.syncWithLifecycle(event.state)
''',
'''        if (event.category === "app_lifecycle") {
            if (["ready", "degraded", "fatal_error", "quarantined", "audit_quarantined",
                 "key_invalidated", "wrapped_key_corrupt", "database_open_failed",
                 "reset_required"].indexOf(event.state) >= 0)
                startupWatchdog.stop()
            NavigationStore.syncWithLifecycle(event.state)
''', '"reset_required"].indexOf(event.state)')
    return text


def patch_life(text):
    text = replace_once(text,
'''        switch (value) {
        case "needs_database_key": return qsTr("Preparing secure storage")
''',
'''        switch (value) {
        case "bootstrapping": return qsTr("Starting Mukei")
        case "initialize_submitted": return qsTr("Submitting secure startup")
        case "initialize_acknowledged": return qsTr("Starting native runtime")
        case "booting": return qsTr("Starting local runtime")
        case "loading_config": return qsTr("Loading private configuration")
        case "needs_database_key": return qsTr("Preparing secure storage")
''', 'Submitting secure startup')
    text = replace_once(text,
'''        switch (value) {
        case "needs_database_key": return qsTr("Waiting for the native secure-key provider. No private data is opened yet.")
''',
'''        switch (value) {
        case "bootstrapping": return qsTr("Connecting the production frontend to the local runtime.")
        case "initialize_submitted": return qsTr("The frontend has formed a validated protocol command for local initialization.")
        case "initialize_acknowledged": return qsTr("The bridge accepted the command. The next expected signal is the native boot stage.")
        case "booting": return qsTr("The native runtime is starting on this device.")
        case "loading_config": return qsTr("Mukei is validating app-private paths and local configuration.")
        case "needs_database_key": return qsTr("Waiting for the native secure-key provider. No private data is opened yet.")
''', 'The bridge accepted the command')
    return text


def patch_rust(text):
    text = replace_once(text,
'async fn hydrate_provider_secrets_from_platform() -> Result<(), String> {',
'fn hydrate_provider_secrets_from_platform() -> Result<(), String> {',
'\nfn hydrate_provider_secrets_from_platform()')
    text = replace_once(text,
'''    Ok(())
}

#[cfg(feature = "rusqlite")]
async fn persist_provider_secret_refs(
''',
'''    Ok(())
}

async fn hydrate_provider_secrets_bounded() -> Result<(), String> {
    match tokio::time::timeout(
        std::time::Duration::from_secs(15),
        tokio::task::spawn_blocking(hydrate_provider_secrets_from_platform),
    ).await {
        Ok(joined) => joined.map_err(|error| format!("provider secret worker failed: {error}"))?,
        Err(_) => Err("provider secret hydration timed out".to_string()),
    }
}

#[cfg(feature = "rusqlite")]
async fn persist_provider_secret_refs(
''', 'hydrate_provider_secrets_bounded')
    text = replace_once(text,
'hydrate_provider_secrets_from_platform().await',
'hydrate_provider_secrets_bounded().await',
'hydrate_provider_secrets_bounded().await')
    old = '''                        let qt_for_state = qt.clone();
                        let prepared = prepare_database_key_with_observer(
                            runtime_state().secure_bootstrap(),
                            &PlatformSecureKeyProvider,
                            move |secure_state| {
'''
    new = '''                        let _ = qt.queue(|mut qobject| {
                            qobject.as_mut().event_emitted(event_json(BridgeEvent::new(
                                BridgeEventKind::AppLifecycle {
                                    state: AppLifecycleState::CreatingWrappingKey,
                                    capabilities: CapabilitySnapshot::uninitialized(),
                                    android_storage: Some(AndroidStorageState::Unknown),
                                },
                            )));
                        });
                        let qt_for_state = qt.clone();
                        let prepared = tokio::task::block_in_place(|| prepare_database_key_with_observer(
                            runtime_state().secure_bootstrap(),
                            &PlatformSecureKeyProvider,
                            move |secure_state| {
'''
    text = replace_once(text, old, new, 'tokio::task::block_in_place(|| prepare_database_key')
    text = replace_once(text,
'''                            },
                        );
                        match prepared {
''',
'''                            },
                        ));
                        match prepared {
''', '));\n                        match prepared')
    return text


def patch_protocol(text):
    text = replace_once(text,
'''/// Parse, structurally validate, policy-preflight, replay-check, and dispatch one command.
pub(crate) fn submit_command_json(
''',
'''fn dispatch_on_owning_qt_thread(command_type: &CommandType) -> bool {
    matches!(command_type, CommandType::AppInitialize)
}

/// Parse, structurally validate, policy-preflight, replay-check, and dispatch one command.
pub(crate) fn submit_command_json(
''', 'dispatch_on_owning_qt_thread')
    text = replace_once(text,
'''    // Return the acknowledgement from the acceptance boundary before execution can emit a
    // completion event. Dispatch remains on the existing QObjects/runtime owner and adapts into
    // the existing backend methods; no second runtime or domain implementation is introduced.
    let qt = agent.as_ref().get_ref().qt_thread();
''',
'''    // Startup is submitted by QML on the owning Qt thread. A second queued Qt hop can be
    // indefinitely delayed on some Android event-loop integrations, leaving the frontend at the
    // acknowledgement boundary without ever entering MukeiAgent::initialize. Dispatch startup
    // directly; initialize itself only schedules bounded native work and returns immediately.
    if dispatch_on_owning_qt_thread(&command.command_type) {
        dispatch_validated_command(agent, command, context);
        return acknowledgement_json(acknowledgement);
    }

    // Other commands preserve the queued dispatch boundary so immediate acknowledgements remain
    // independent from operation completion events.
    let qt = agent.as_ref().get_ref().qt_thread();
''', 'A second queued Qt hop can be')
    text = replace_once(text,
'''    #[test]
    fn sol02_idempotent_replay_rebinds_transport_ids() {
''',
'''    #[test]
    fn app_initialize_dispatches_without_a_second_qt_queue() {
        assert!(dispatch_on_owning_qt_thread(&CommandType::AppInitialize));
        assert!(!dispatch_on_owning_qt_thread(&CommandType::ChatSendMessage));
    }

    #[test]
    fn sol02_idempotent_replay_rebinds_transport_ids() {
''', 'app_initialize_dispatches_without_a_second_qt_queue')
    return text


APP.write_text(patch_app(APP.read_text()), encoding="utf-8")
LIFE.write_text(patch_life(LIFE.read_text()), encoding="utf-8")
RUST.write_text(patch_rust(RUST.read_text()), encoding="utf-8")
PROTOCOL.write_text(patch_protocol(PROTOCOL.read_text()), encoding="utf-8")
install_stabilization_batch()
print("startup bootstrap source finalized")
