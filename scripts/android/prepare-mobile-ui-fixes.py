#!/usr/bin/env python3
"""Apply mobile-shell fixes after the full-QML and startup-event patches.

These changes are safe for the diagnostic stub build:
- local navigation no longer depends on backend protocol objects;
- the compact drawer occupies the full Android window height;
- the diagnostics fallback remains available but is not shown on success;
- compact chat content gets additional top spacing away from system chrome.
"""
from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
INTENT_DISPATCHER = ROOT / "qml/architecture/IntentDispatcher.qml"
LEFT_DRAWER = ROOT / "qml/components/LeftDrawer.qml"
CHAT_SCREEN = ROOT / "qml/screens/ChatScreen.qml"
MAIN = ROOT / "qml/main.cpp"
MANIFEST = ROOT / "qml/android/AndroidManifest.xml"


def patch_intent_dispatcher() -> None:
    text = INTENT_DISPATCHER.read_text(encoding="utf-8")
    anchor = '''    function dispatch(intent) {
        if (!intent || typeof intent !== "object" || typeof intent.type !== "string")
            return reject("ERR_UI_INVALID_INTENT", qsTr("That action was not valid."), intent)
        if (contractStoreRef === null || capabilityStoreRef === null || chatStoreRef === null || operationStoreRef === null)
            return reject("ERR_UI_DISPATCH_DEPENDENCY", qsTr("The local UI state machine is not ready."), intent)

        try {
'''
    replacement = '''    function dispatch(intent) {
        if (!intent || typeof intent !== "object" || typeof intent.type !== "string")
            return reject("ERR_UI_INVALID_INTENT", qsTr("That action was not valid."), intent)

        // Navigation is a local presentation concern. It must remain usable even
        // while the native bridge is unavailable, negotiating, or in recovery.
        if (intent.type === "navigation.open") {
            if (!NavigationStore.navigate(intent.route, intent.parameters || ({}), intent.replace === true))
                return false
            intentAccepted(intent.type)
            return true
        }
        if (intent.type === "navigation.back") {
            if (!NavigationStore.goBack())
                return false
            intentAccepted(intent.type)
            return true
        }

        if (contractStoreRef === null || capabilityStoreRef === null || chatStoreRef === null || operationStoreRef === null)
            return reject("ERR_UI_DISPATCH_DEPENDENCY", qsTr("The local UI state machine is not ready."), intent)

        try {
'''
    if anchor not in text:
        raise SystemExit("IntentDispatcher dispatch anchor not found")
    text = text.replace(anchor, replacement, 1)
    INTENT_DISPATCHER.write_text(text, encoding="utf-8")


def patch_drawer() -> None:
    text = LEFT_DRAWER.read_text(encoding="utf-8")
    anchor = '''Drawer {
    id: root
    width: Type.compact ? Spacing.huge * 3 - Spacing.xs : Spacing.huge * 3 + Spacing.xl
    edge: Qt.LeftEdge
'''
    replacement = '''Drawer {
    id: root
    width: Math.min(parent ? parent.width * 0.86 : Spacing.huge * 3,
                    Type.compact ? Spacing.huge * 4 : Spacing.huge * 5)
    height: parent ? parent.height : implicitHeight
    edge: Qt.LeftEdge
    modal: true
    interactive: true
'''
    if anchor not in text:
        raise SystemExit("LeftDrawer geometry anchor not found")
    text = text.replace(anchor, replacement, 1)
    LEFT_DRAWER.write_text(text, encoding="utf-8")


def patch_chat_screen() -> None:
    text = CHAT_SCREEN.read_text(encoding="utf-8")
    anchor = '''            Layout.margins: Spacing.md
            spacing: Spacing.sm
'''
    replacement = '''            Layout.leftMargin: Spacing.md
            Layout.rightMargin: Spacing.md
            Layout.topMargin: ResponsiveStore.compact ? Spacing.lg : Spacing.md
            Layout.bottomMargin: ResponsiveStore.compact ? Spacing.lg : Spacing.md
            spacing: Spacing.sm
'''
    if anchor not in text:
        raise SystemExit("ChatScreen margin anchor not found")
    text = text.replace(anchor, replacement, 1)
    CHAT_SCREEN.write_text(text, encoding="utf-8")


def patch_generated_main() -> None:
    text = MAIN.read_text(encoding="utf-8")
    anchor = '''    diagnosticsLayout->addWidget(diagnosticsText, 1);
    diagnosticsWindow.showFullScreen();

    QQmlApplicationEngine engine;
'''
    replacement = '''    diagnosticsLayout->addWidget(diagnosticsText, 1);
    // Keep the fallback window hidden unless QML creation actually fails.

    QQmlApplicationEngine engine;
'''
    if anchor not in text:
        raise SystemExit("generated diagnostics visibility anchor not found")
    text = text.replace(anchor, replacement, 1)
    MAIN.write_text(text, encoding="utf-8")


def patch_manifest() -> None:
    text = MANIFEST.read_text(encoding="utf-8")
    text = text.replace('android:label="Mukei UI Runtime Fix"', 'android:label="Mukei Mobile UI Fix"')
    MANIFEST.write_text(text, encoding="utf-8")


def main() -> int:
    patch_intent_dispatcher()
    patch_drawer()
    patch_chat_screen()
    patch_generated_main()
    patch_manifest()
    print("Patched bridge-independent navigation, full-height drawer, safe spacing, and hidden diagnostics")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
