#!/usr/bin/env python3
from pathlib import Path

from install_stabilization_batch import main as install_stabilization_batch

ROOT = Path(__file__).resolve().parents[2]
APP = ROOT / "qml/architecture/AppCoordinator.qml"
LIFE = ROOT / "qml/stores/LifecycleStore.qml"
RUST = ROOT / "rust/crates/mukei-bridge/src/lib.rs"


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
        case "booting": return qsTr("Starting local runtime")
        case "loading_config": return qsTr("Loading private configuration")
        case "needs_database_key": return qsTr("Preparing secure storage")
''', 'Loading private configuration')
    text = replace_once(text,
'''        switch (value) {
        case "needs_database_key": return qsTr("Waiting for the native secure-key provider. No private data is opened yet.")
''',
'''        switch (value) {
        case "bootstrapping": return qsTr("Connecting the production frontend to the local runtime.")
        case "booting": return qsTr("The native runtime is starting on this device.")
        case "loading_config": return qsTr("Mukei is validating app-private paths and local configuration.")
        case "needs_database_key": return qsTr("Waiting for the native secure-key provider. No private data is opened yet.")
''', 'validating app-private paths')
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


APP.write_text(patch_app(APP.read_text()), encoding="utf-8")
LIFE.write_text(patch_life(LIFE.read_text()), encoding="utf-8")
RUST.write_text(patch_rust(RUST.read_text()), encoding="utf-8")
install_stabilization_batch()
print("startup bootstrap source finalized")
