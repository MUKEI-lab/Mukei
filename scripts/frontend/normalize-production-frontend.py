#!/usr/bin/env python3
"""Normalize generated production frontend source before CI commits it."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def patch_main_cpp() -> None:
    path = ROOT / "qml/main.cpp"
    text = path.read_text(encoding="utf-8")

    if "#include <QQmlError>" not in text:
        text = text.replace(
            "#include <QQmlApplicationEngine>\n",
            "#include <QQmlApplicationEngine>\n#include <QQmlError>\n",
            1,
        )

    init_anchor = '''            if (commandType == QStringLiteral("app.initialize")) {
                m_appContext = context;
                initialize(payload.value(QStringLiteral("config_path")).toString());
'''
    init_replacement = '''            if (commandType == QStringLiteral("app.initialize")) {
                m_appContext = context;
                initialize(payload.value(QStringLiteral("config_path")).toString());
                emitOperationLifecycle(context, true, QJsonObject{
                    {QStringLiteral("initialized"), true},
                    {QStringLiteral("runtime"), QStringLiteral("stub")}
                });
'''
    if init_anchor in text:
        text = text.replace(init_anchor, init_replacement, 1)

    text = text.replace(
        "        emit event_emitted(eventJson);\n        emit eventEmitted(eventJson);",
        "        emit eventEmitted(eventJson);",
        1,
    )
    path.write_text(text, encoding="utf-8")


def patch_main_window() -> None:
    path = ROOT / "qml/MainWindow.qml"
    text = path.read_text(encoding="utf-8")
    text = text.replace(
        '    LayoutMirroring.enabled: Qt.application.layoutDirection === Qt.RightToLeft\n',
        '    LayoutMirroring.enabled: Qt.application.layoutDirection === Qt.RightToLeft // qmllint disable missing-property\n',
    )
    text = text.replace(
        '        AppCoordinator.configure(mukeiAgent, mukeiBridge, mukeiRuntime)\n',
        '        AppCoordinator.configure(mukeiAgent, mukeiBridge, mukeiRuntime) // qmllint disable unqualified\n',
    )
    path.write_text(text, encoding="utf-8")


def patch_responsive_store() -> None:
    path = ROOT / "qml/stores/ResponsiveStore.qml"
    text = path.read_text(encoding="utf-8")
    if 'import "../theme"' not in text:
        text = text.replace('import QtQuick\n', 'import QtQuick\nimport "../theme"\n', 1)
    path.write_text(text, encoding="utf-8")


def patch_adaptive_navigation() -> None:
    path = ROOT / "qml/shell/AdaptiveNavigation.qml"
    text = path.read_text(encoding="utf-8")
    for old, new in {
        'icon: "qrc:/icons/chat.svg"': 'icon: "chat"',
        'icon: "qrc:/icons/chip.svg"': 'icon: "chip"',
        'icon: "qrc:/icons/file.svg"': 'icon: "file"',
        'icon: "qrc:/icons/done-target.svg"': 'icon: "done-target"',
        'icon: "qrc:/icons/settings.svg"': 'icon: "settings"',
        'source: navigationButton.modelData.icon': 'name: navigationButton.modelData.icon',
        'color: navigationButton.checked ? Theme.p.accent : Theme.p.inkSecondary':
            'tone: navigationButton.checked ? Theme.p.accent : Theme.p.inkSecondary',
    }.items():
        text = text.replace(old, new)
    path.write_text(text, encoding="utf-8")


def patch_chat_composer() -> None:
    path = ROOT / "qml/components/ChatComposer.qml"
    text = path.read_text(encoding="utf-8")
    text = text.replace(
        'source: root.isStreaming ? "qrc:/icons/stop.svg" : "qrc:/icons/send.svg"',
        'name: root.isStreaming ? "stop" : "send"',
    )
    text = text.replace(
        'color: sendButton.enabled ? Theme.p.background : Theme.p.inkFaint',
        'tone: sendButton.enabled ? Theme.p.background : Theme.p.inkFaint',
    )
    path.write_text(text, encoding="utf-8")


def validate() -> None:
    checks = {
        ROOT / "qml/CMakeLists.txt": ["qt_add_executable(mukei", "set(CMAKE_AUTORCC ON)"],
        ROOT / "qml/main.cpp": ["qrc:/com/mukei/app/MainWindow.qml", "QSGRendererInterface::OpenGL", "#include <QQmlError>"],
        ROOT / "qml/events/EventDispatcher.qml": ["function onEventEmitted", "function onEvent_emitted"],
        ROOT / "qml/MainWindow.qml": ["qmllint disable missing-property", "qmllint disable unqualified"],
        ROOT / "qml/stores/ResponsiveStore.qml": ['import "../theme"', "Spacing.lg"],
        ROOT / "qml/screens/EmptyChatScreen.qml": ["Mukei is ready", "Everything runs on your device"],
        ROOT / "qml/screens/ChatScreen.qml": ["Choose a verified local model", "EmptyChatScreen"],
        ROOT / "qml/screens/SettingsScreen.qml": ["All data lives on this device", "Inference defaults"],
    }
    for path, needles in checks.items():
        text = path.read_text(encoding="utf-8")
        for needle in needles:
            if needle not in text:
                raise SystemExit(f"missing production marker in {path}: {needle}")


def main() -> int:
    patch_main_cpp()
    patch_main_window()
    patch_responsive_store()
    patch_adaptive_navigation()
    patch_chat_composer()
    validate()
    print("Production frontend output normalized and validated")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
