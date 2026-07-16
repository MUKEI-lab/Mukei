#!/usr/bin/env python3
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SAFE = ROOT / "qml/screens/SafeModeScreen.qml"
NAV = ROOT / "qml/stores/NavigationStore.qml"
DIAG = ROOT / "qml/screens/DiagnosticsScreen.qml"

safe = SAFE.read_text(encoding="utf-8")
if 'onClicked: {' not in safe:
    safe = safe.replace('import "../theme"\n', 'import "../architecture"\nimport "../stores"\nimport "../theme"\n', 1)
    safe = safe.replace('''        PrimaryButton {
            Layout.fillWidth: true
            text: qsTr("Continue Anyway")
        }
''', '''        PrimaryButton {
            Layout.fillWidth: true
            text: qsTr("Continue Anyway")
            onClicked: {
                ErrorStore.dismiss()
                LifecycleStore.setLocalState("degraded", qsTr("Mukei is open in limited mode because native startup did not finish."))
                NavigationStore.syncWithLifecycle(LifecycleStore.state)
            }
        }
''', 1)
    safe = safe.replace('''        DestructiveButton {
            Layout.fillWidth: true
            text: qsTr("Reset All Data")
        }
''', '''        DestructiveButton {
            Layout.fillWidth: true
            text: qsTr("Reset All Data")
            onClicked: ErrorStore.push({
                code: "ERR_RESET_REQUIRES_REINSTALL",
                severity: "error",
                recoverable: true,
                user_message: qsTr("Automatic reset is not available in this build. Uninstall Mukei, then install the corrected APK.")
            }, "ERR_RESET_REQUIRES_REINSTALL")
        }
''', 1)
    safe = safe.replace('''        GhostButton {
            text: qsTr("View Crash Log")
        }
''', '''        GhostButton {
            text: qsTr("View Crash Log")
            onClicked: {
                ErrorStore.dismiss()
                NavigationStore.navigate("diagnostics", ({ from: "safe_mode" }), false)
            }
        }
''', 1)
SAFE.write_text(safe, encoding="utf-8")

nav = NAV.read_text(encoding="utf-8")
nav = nav.replace('["boot", "unlock", "welcome", "security", "compatibility"].indexOf(route)',
                  '["boot", "unlock", "welcome", "security", "diagnostics", "compatibility"].indexOf(route)')
NAV.write_text(nav, encoding="utf-8")

diag = DIAG.read_text(encoding="utf-8")
if 'Last startup stage' not in diag:
    diag = diag.replace('{ label: qsTr("Runtime"), value: DiagnosticsStore.snapshot.runtime_phase || LifecycleStore.state },',
                        '{ label: qsTr("Runtime"), value: DiagnosticsStore.snapshot.runtime_phase || LifecycleStore.state },\n        { label: qsTr("Last startup stage"), value: LifecycleStore.previousState || LifecycleStore.state },', 1)
diag = diag.replace('enabled: !DiagnosticsStore.exporting && LifecycleStore.interactive',
                    'enabled: !DiagnosticsStore.exporting')
DIAG.write_text(diag, encoding="utf-8")

print("safe-mode recovery actions finalized")
