#!/usr/bin/env python3
"""Apply Android 15 edge-to-edge insets and deterministic local navigation.

Qt 6.8 does not expose the SafeArea QML attached type (introduced in Qt 6.9),
so this diagnostic Android build uses conservative dp insets. It also routes
settings/back actions directly through NavigationStore so they cannot be
blocked by bridge protocol state.
"""
from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MAIN_WINDOW = ROOT / "qml/MainWindow.qml"
CHAT_SCREEN = ROOT / "qml/screens/ChatScreen.qml"
LEFT_DRAWER = ROOT / "qml/components/LeftDrawer.qml"
SETTINGS_SCREEN = ROOT / "qml/screens/SettingsScreen.qml"
MANIFEST = ROOT / "qml/android/AndroidManifest.xml"


def patch_main_window() -> None:
    text = MAIN_WINDOW.read_text(encoding="utf-8")
    anchor = '''    AppShell {
        anchors.fill: parent
    }
'''
    replacement = '''    readonly property real androidTopInset: Qt.platform.os === "android" ? 28 : 0
    readonly property real androidBottomInset: Qt.platform.os === "android" ? 28 : 0

    AppShell {
        anchors.fill: parent
        anchors.topMargin: root.androidTopInset
        anchors.bottomMargin: root.androidBottomInset
    }
'''
    if anchor not in text:
        raise SystemExit("MainWindow AppShell anchor not found")
    text = text.replace(anchor, replacement, 1)

    shortcut = '''        onActivated: IntentDispatcher.dispatch({
            type: "navigation.open",
            route: "settings"
        })
'''
    direct = '''        onActivated: {
            NavigationStore.lifecycleLocked = false
            NavigationStore.navigate("settings", ({}), false)
        }
'''
    if shortcut not in text:
        raise SystemExit("MainWindow settings shortcut anchor not found")
    text = text.replace(shortcut, direct, 1)

    back = '        onActivated: IntentDispatcher.dispatch({ type: "navigation.back" })\n'
    back_direct = '        onActivated: NavigationStore.goBack()\n'
    if back not in text:
        raise SystemExit("MainWindow back shortcut anchor not found")
    text = text.replace(back, back_direct, 1)
    MAIN_WINDOW.write_text(text, encoding="utf-8")


def patch_chat_screen() -> None:
    text = CHAT_SCREEN.read_text(encoding="utf-8")
    anchor = '                    onClicked: IntentDispatcher.dispatch({ type: "navigation.open", route: "settings" })\n'
    replacement = '''                    onClicked: {
                        NavigationStore.lifecycleLocked = false
                        NavigationStore.navigate("settings", ({}), false)
                    }
'''
    if anchor not in text:
        raise SystemExit("ChatScreen settings button anchor not found")
    text = text.replace(anchor, replacement, 1)
    CHAT_SCREEN.write_text(text, encoding="utf-8")


def patch_drawer() -> None:
    text = LEFT_DRAWER.read_text(encoding="utf-8")
    root_anchor = '''Drawer {
    id: root
'''
    root_replacement = '''Drawer {
    id: root
    readonly property real androidTopInset: Qt.platform.os === "android" ? 28 : 0
    readonly property real androidBottomInset: Qt.platform.os === "android" ? 32 : 0
'''
    if root_anchor not in text:
        raise SystemExit("LeftDrawer root anchor not found")
    text = text.replace(root_anchor, root_replacement, 1)

    margins = '''        anchors.fill: parent
        anchors.margins: Spacing.md
        spacing: Spacing.md
'''
    safe_margins = '''        anchors.fill: parent
        anchors.leftMargin: Spacing.md
        anchors.rightMargin: Spacing.md
        anchors.topMargin: Spacing.md + root.androidTopInset
        anchors.bottomMargin: Spacing.md + root.androidBottomInset
        spacing: Spacing.md
'''
    if margins not in text:
        raise SystemExit("LeftDrawer content margin anchor not found")
    text = text.replace(margins, safe_margins, 1)

    settings = '''            onClicked: {
                IntentDispatcher.dispatch({ type: "navigation.open", route: "settings" })
                root.close()
            }
'''
    settings_direct = '''            onClicked: {
                NavigationStore.lifecycleLocked = false
                NavigationStore.navigate("settings", ({}), false)
                root.close()
            }
'''
    if settings not in text:
        raise SystemExit("LeftDrawer settings action anchor not found")
    text = text.replace(settings, settings_direct, 1)
    LEFT_DRAWER.write_text(text, encoding="utf-8")


def patch_settings_screen() -> None:
    text = SETTINGS_SCREEN.read_text(encoding="utf-8")
    anchor = '                onClicked: IntentDispatcher.dispatch({ type: "navigation.back" })\n'
    replacement = '                onClicked: NavigationStore.goBack()\n'
    if anchor not in text:
        raise SystemExit("SettingsScreen back anchor not found")
    text = text.replace(anchor, replacement, 1)
    SETTINGS_SCREEN.write_text(text, encoding="utf-8")


def patch_manifest() -> None:
    text = MANIFEST.read_text(encoding="utf-8")
    text = text.replace('android:label="Mukei Mobile UI Fix"', 'android:label="Mukei Insets Navigation Fix"')
    MANIFEST.write_text(text, encoding="utf-8")


def main() -> int:
    patch_main_window()
    patch_chat_screen()
    patch_drawer()
    patch_settings_screen()
    patch_manifest()
    print("Applied Android inset fallback and deterministic local navigation")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
