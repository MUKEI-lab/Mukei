#!/usr/bin/env python3
"""Promote proven Android/QML fixes into canonical production source.

The migration is idempotent. It converts the product target to Qt's Android-aware
executable, removes build-time bootstrap patching, permanently wires the CXX-Qt
event protocol, and installs the UXB v2.1 mobile shell.
"""
from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def write(path: Path, content: str) -> None:
    path.write_text(content.rstrip() + "\n", encoding="utf-8")


def patch_cmake() -> None:
    path = ROOT / "qml/CMakeLists.txt"
    text = path.read_text(encoding="utf-8")
    if "set(CMAKE_AUTORCC ON)" not in text:
        text = text.replace(
            "set(CMAKE_AUTOMOC ON)\n",
            "set(CMAKE_AUTOMOC ON)\nset(CMAKE_AUTORCC ON)\n",
            1,
        )
    text = text.replace("\nadd_executable(mukei\n", "\nqt_add_executable(mukei\n", 1)
    android_block = """
if(ANDROID)
    set_property(TARGET mukei PROPERTY QT_ANDROID_MIN_SDK_VERSION 29)
    set_property(TARGET mukei PROPERTY QT_ANDROID_TARGET_SDK_VERSION 35)
    set_property(TARGET mukei PROPERTY QT_ANDROID_PACKAGE_SOURCE_DIR
        ${CMAKE_CURRENT_SOURCE_DIR}/android
    )
endif()
"""
    marker = "\ntarget_link_libraries(mukei PRIVATE\n"
    if "QT_ANDROID_PACKAGE_SOURCE_DIR" not in text:
        if marker not in text:
            raise SystemExit("CMake target_link_libraries anchor missing")
        text = text.replace(marker, android_block + marker, 1)
    path.write_text(text, encoding="utf-8")


def patch_build_script() -> None:
    path = ROOT / "scripts/android/build-apk.sh"
    text = path.read_text(encoding="utf-8")
    start = text.find("# The shared QML project historically used plain add_executable()")
    end = text.find("printf '\\n==> Building llama.cpp capsule", start)
    if start >= 0 and end >= 0:
        replacement = """# Stage an isolated source mirror so Android packaging never mutates the
# repository checkout. Canonical CMake now uses qt_add_executable() directly;
# no source transformation is permitted here.
rm -rf "${ANDROID_SOURCE_ROOT}"
mkdir -p "${ANDROID_SOURCE_ROOT}"
cp -a "${REPO_ROOT}/qml" "${ANDROID_QML_SOURCE_DIR}"
ln -s "${REPO_ROOT}/rust" "${ANDROID_SOURCE_ROOT}/rust"

"""
        text = text[:start] + replacement + text[end:]
    path.write_text(text, encoding="utf-8")


def patch_main_cpp() -> None:
    path = ROOT / "qml/main.cpp"
    text = path.read_text(encoding="utf-8")
    text = text.replace("QSGRendererInterface::Vulkan", "QSGRendererInterface::OpenGL")

    app_anchor = "    QGuiApplication app(argc, argv);\n"
    if "QCoreApplication::setOrganizationName" not in text:
        if app_anchor not in text:
            raise SystemExit("main.cpp application anchor missing")
        text = text.replace(
            app_anchor,
            app_anchor
            + "    QCoreApplication::setOrganizationName(QStringLiteral(\"MUKEI-lab\"));\n"
            + "    QCoreApplication::setOrganizationDomain(QStringLiteral(\"mukei.app\"));\n"
            + "    QCoreApplication::setApplicationName(QStringLiteral(\"Mukei\"));\n"
            + "    QCoreApplication::setApplicationVersion(QString::fromLatin1(MUKEI_PRODUCT_VERSION));\n",
            1,
        )

    bridge_anchor = """#endif

    QQmlApplicationEngine engine;
"""
    if "const QString modelsPath" not in text:
        replacement = """#endif

    const QString modelsPath = QDir(runtimeInfo.appDataPath()).filePath(QStringLiteral("models"));
    QDir().mkpath(modelsPath);
    bridge.set_model_dir(modelsPath);

    QQmlApplicationEngine engine;
"""
        if bridge_anchor not in text:
            raise SystemExit("main.cpp bridge setup anchor missing")
        text = text.replace(bridge_anchor, replacement, 1)

    old_load = """    QObject::connect(&engine, &QQmlApplicationEngine::objectCreationFailed, &app, [] {
        QCoreApplication::exit(-1);
    }, Qt::QueuedConnection);

    QTimer::singleShot(100, &engine, [&engine] {
        engine.load(QUrl(QStringLiteral("qrc:/qt/qml/com/mukei/app/MainWindow.qml")));
    });
"""
    new_load = """    const QUrl mainWindowUrl(QStringLiteral("qrc:/com/mukei/app/MainWindow.qml"));
    QObject::connect(&engine, &QQmlApplicationEngine::warnings, &app,
                     [](const QList<QQmlError> &warnings) {
        for (const QQmlError &warning : warnings)
            qCritical().noquote() << "MukeiQml" << warning.toString();
    });
    QObject::connect(&engine, &QQmlApplicationEngine::objectCreated, &app,
                     [mainWindowUrl](QObject *object, const QUrl &url) {
        qInfo().noquote() << "MukeiStartup root_object"
                          << (object ? "ready" : "failed")
                          << url.toString();
        if (!object && url == mainWindowUrl)
            QCoreApplication::exit(EXIT_FAILURE);
    }, Qt::QueuedConnection);
    engine.load(mainWindowUrl);
"""
    if old_load in text:
        text = text.replace(old_load, new_load, 1)
    elif new_load not in text:
        raise SystemExit("main.cpp QML load block missing")

    signal_anchor = (
        "    void event_emitted(const QString &eventJson);\n"
        "    void async_result(const QString &resultJson);"
    )
    if signal_anchor in text:
        text = text.replace(
            signal_anchor,
            "    void event_emitted(const QString &eventJson);\n"
            "    void eventEmitted(const QString &eventJson);\n"
            "    void async_result(const QString &resultJson);",
            1,
        )

    emit_anchor = (
        "        emit event_emitted(QString::fromUtf8("
        "QJsonDocument(event).toJson(QJsonDocument::Compact)));"
    )
    if emit_anchor in text:
        text = text.replace(
            emit_anchor,
            "        const QString eventJson = QString::fromUtf8(\n"
            "            QJsonDocument(event).toJson(QJsonDocument::Compact));\n"
            "        emit event_emitted(eventJson);\n"
            "        emit eventEmitted(eventJson);",
            1,
        )

    contract_anchor = '"operation_lifecycle_events","legacy_event_v1_compatibility"'
    text = text.replace(
        contract_anchor,
        '"operation_lifecycle_events","scoped_chat_operations","legacy_event_v1_compatibility"',
    )
    path.write_text(text, encoding="utf-8")


def patch_event_dispatcher() -> None:
    path = ROOT / "qml/events/EventDispatcher.qml"
    text = path.read_text(encoding="utf-8")
    text = text.replace(
        """    Connections {
        target: root.agentSource === null ? null : root.agentSource
        function onEvent_emitted(eventJson) { root.ingest(eventJson, "agent") }
    }
""",
        """    Connections {
        target: root.agentSource === null ? null : root.agentSource
        ignoreUnknownSignals: true
        function onEvent_emitted(eventJson) { root.ingest(eventJson, "agent") }
        function onEventEmitted(eventJson) { root.ingest(eventJson, "agent") }
    }
""",
    )
    text = text.replace(
        """    Connections {
        target: root.bridgeSource === null ? null : root.bridgeSource
        function onEvent_emitted(eventJson) { root.ingest(eventJson, "bridge") }
    }
""",
        """    Connections {
        target: root.bridgeSource === null ? null : root.bridgeSource
        ignoreUnknownSignals: true
        function onEvent_emitted(eventJson) { root.ingest(eventJson, "bridge") }
        function onEventEmitted(eventJson) { root.ingest(eventJson, "bridge") }
    }
""",
    )
    path.write_text(text, encoding="utf-8")


def patch_intent_dispatcher() -> None:
    path = ROOT / "qml/architecture/IntentDispatcher.qml"
    text = path.read_text(encoding="utf-8")
    anchor = """    function dispatch(intent) {
        if (!intent || typeof intent !== "object" || typeof intent.type !== "string")
            return reject("ERR_UI_INVALID_INTENT", qsTr("That action was not valid."), intent)
        if (contractStoreRef === null || capabilityStoreRef === null || chatStoreRef === null || operationStoreRef === null)
            return reject("ERR_UI_DISPATCH_DEPENDENCY", qsTr("The local UI state machine is not ready."), intent)

        try {
"""
    replacement = """    function dispatch(intent) {
        if (!intent || typeof intent !== "object" || typeof intent.type !== "string")
            return reject("ERR_UI_INVALID_INTENT", qsTr("That action was not valid."), intent)

        // Navigation is local presentation state and never depends on the
        // native command protocol being available.
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
"""
    if replacement not in text:
        if anchor not in text:
            raise SystemExit("IntentDispatcher dispatch anchor missing")
        text = text.replace(anchor, replacement, 1)
    text = text.replace(
        """            case "navigation.open":
                if (!NavigationStore.navigate(intent.route, intent.parameters || ({}), intent.replace === true))
                    return false
                break
            case "navigation.back":
                if (!NavigationStore.goBack())
                    return false
                break
""",
        "",
        1,
    )
    path.write_text(text, encoding="utf-8")


def patch_responsive_store() -> None:
    write(
        ROOT / "qml/stores/ResponsiveStore.qml",
        """pragma Singleton
import QtQuick

QtObject {
    enum Mode { Compact, Medium, Expanded }

    property real viewportWidth: 0
    property real viewportHeight: 0

    readonly property int mode: viewportWidth < 600
                                ? ResponsiveStore.Mode.Compact
                                : viewportWidth < 840
                                  ? ResponsiveStore.Mode.Medium
                                  : ResponsiveStore.Mode.Expanded
    readonly property bool compact: mode === ResponsiveStore.Mode.Compact
    readonly property bool medium: mode === ResponsiveStore.Mode.Medium
    readonly property bool expanded: mode === ResponsiveStore.Mode.Expanded
    readonly property real edgePadding: compact ? Spacing.lg : medium ? Spacing.xl : Spacing.xxl
    readonly property real contentMaxWidth: expanded ? 960 : medium ? 760 : Math.max(0, viewportWidth)

    function updateViewport(width, height) {
        viewportWidth = Math.max(0, width || 0)
        viewportHeight = Math.max(0, height || 0)
    }
}
""",
    )


def write_main_window() -> None:
    write(
        ROOT / "qml/MainWindow.qml",
        """import QtQuick
import QtQuick.Controls.Basic
import "architecture"
import "stores"
import "shell"
import "theme"

ApplicationWindow {
    id: root

    visible: true
    width: Spacing.huge * 4
    height: Spacing.huge * 8 + Spacing.xxl
    minimumWidth: Spacing.huge * 3
    minimumHeight: Spacing.huge * 5
    color: Theme.p.background
    title: qsTr("Mukei")

    readonly property real safeTop: Qt.platform.os === "android" ? 28 : 0
    readonly property real safeBottom: Qt.platform.os === "android" ? 32 : 0
    readonly property real safeLeft: 0
    readonly property real safeRight: 0

    Behavior on color {
        enabled: !Theme.reduceMotion
        ColorAnimation { duration: Motion.themeCrossFade; easing.type: Easing.OutCubic }
    }

    LayoutMirroring.enabled: Qt.application.layoutDirection === Qt.RightToLeft
    LayoutMirroring.childrenInherit: true

    signal accessibilityAnnouncementRequested(string text)

    Rectangle {
        anchors.fill: parent
        color: Theme.p.background
    }

    AppShell {
        anchors.fill: parent
        anchors.topMargin: root.safeTop
        anchors.bottomMargin: root.safeBottom
        anchors.leftMargin: root.safeLeft
        anchors.rightMargin: root.safeRight
    }

    onWidthChanged: ResponsiveStore.updateViewport(width - safeLeft - safeRight,
                                                    height - safeTop - safeBottom)
    onHeightChanged: ResponsiveStore.updateViewport(width - safeLeft - safeRight,
                                                     height - safeTop - safeBottom)

    Component.onCompleted: {
        ResponsiveStore.updateViewport(width - safeLeft - safeRight,
                                       height - safeTop - safeBottom)
        AppCoordinator.configure(mukeiAgent, mukeiBridge, mukeiRuntime)
        AppCoordinator.start()
    }

    Connections {
        target: AccessibilityStore
        function onAnnouncementReady(text) {
            root.accessibilityAnnouncementRequested(text)
        }
    }

    Connections {
        target: Qt.application
        function onStateChanged(state) {
            AppCoordinator.onApplicationStateChanged(state)
        }
    }

    Shortcut {
        sequences: [ StandardKey.Preferences ]
        enabled: CapabilityStore.canOpenSettings
        onActivated: IntentDispatcher.dispatch({
            type: "navigation.open",
            route: "settings"
        })
    }

    Shortcut {
        sequences: [ StandardKey.Back ]
        onActivated: IntentDispatcher.dispatch({ type: "navigation.back" })
    }
}
""",
    )


def write_app_shell() -> None:
    write(
        ROOT / "qml/shell/AppShell.qml",
        """import QtQuick

Item {
    id: root
    clip: true

    AdaptiveNavigation {
        id: adaptiveNavigation
        anchors.left: parent.left
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        z: 20
    }

    RouterHost {
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        anchors.right: parent.right
        anchors.left: adaptiveNavigation.visible ? adaptiveNavigation.right : parent.left
    }

    BannerHost {
        anchors.left: adaptiveNavigation.visible ? adaptiveNavigation.right : parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        z: 50
    }

    SnackbarHost {
        anchors.left: adaptiveNavigation.visible ? adaptiveNavigation.right : parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        z: 80
    }

    OperationOverlayHost {
        anchors.top: parent.top
        anchors.right: parent.right
        z: 60
    }

    SheetHost { anchors.fill: parent; z: 70 }
    DialogHost { anchors.fill: parent; z: 90 }
}
""",
    )


def write_adaptive_navigation() -> None:
    write(
        ROOT / "qml/shell/AdaptiveNavigation.qml",
        """import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../architecture"
import "../stores"
import "../theme"
import "../components"

Rectangle {
    id: root

    visible: LifecycleStore.interactive && !ResponsiveStore.compact
    width: ResponsiveStore.expanded ? 264 : 80
    color: Theme.p.surface

    readonly property var destinations: [
        { route: "chat", label: qsTr("Chat"), icon: "qrc:/icons/chat.svg" },
        { route: "models", label: qsTr("Models"), icon: "qrc:/icons/chip.svg" },
        { route: "documents", label: qsTr("Documents"), icon: "qrc:/icons/file.svg" },
        { route: "downloads", label: qsTr("Downloads"), icon: "qrc:/icons/done-target.svg" },
        { route: "settings", label: qsTr("Settings"), icon: "qrc:/icons/settings.svg" }
    ]

    ColumnLayout {
        anchors.fill: parent
        anchors.leftMargin: Spacing.sm
        anchors.rightMargin: Spacing.sm
        anchors.topMargin: Spacing.lg
        anchors.bottomMargin: Spacing.lg
        spacing: Spacing.xs

        Text {
            Layout.fillWidth: true
            Layout.leftMargin: Spacing.sm
            visible: ResponsiveStore.expanded
            text: qsTr("Mukei")
            color: Theme.p.inkPrimary
            Component.onCompleted: Type.apply(this, Type.h2)
        }

        Text {
            Layout.fillWidth: true
            Layout.leftMargin: Spacing.sm
            Layout.bottomMargin: Spacing.md
            visible: ResponsiveStore.expanded
            text: qsTr("Private by construction")
            color: Theme.p.inkSecondary
            Component.onCompleted: Type.apply(this, Type.caption)
        }

        Repeater {
            model: root.destinations
            delegate: Button {
                id: navigationButton
                required property var modelData

                Layout.fillWidth: true
                implicitHeight: 52
                checkable: true
                checked: NavigationStore.currentRoute === navigationButton.modelData.route
                Accessible.name: navigationButton.modelData.label
                onClicked: IntentDispatcher.dispatch({
                    type: "navigation.open",
                    route: navigationButton.modelData.route
                })

                background: Rectangle {
                    radius: Theme.radiusLg
                    color: navigationButton.checked
                           ? Theme.p.surfaceFaint
                           : navigationButton.down || navigationButton.hovered
                             ? Theme.p.surfaceVariant
                             : "transparent"
                    border.width: navigationButton.visualFocus ? 2 : 0
                    border.color: Theme.p.accent

                    Rectangle {
                        visible: navigationButton.checked
                        width: 3
                        height: parent.height - Spacing.md
                        radius: 2
                        anchors.left: parent.left
                        anchors.verticalCenter: parent.verticalCenter
                        color: Theme.p.accent
                    }
                }

                contentItem: RowLayout {
                    spacing: Spacing.sm

                    MukeiIcon {
                        source: navigationButton.modelData.icon
                        color: navigationButton.checked ? Theme.p.accent : Theme.p.inkSecondary
                        Layout.preferredWidth: Spacing.lg
                        Layout.preferredHeight: Spacing.lg
                        Layout.alignment: Qt.AlignHCenter
                    }

                    Text {
                        visible: ResponsiveStore.expanded
                        Layout.fillWidth: true
                        text: navigationButton.modelData.label
                        color: navigationButton.checked ? Theme.p.inkPrimary : Theme.p.inkSecondary
                        Component.onCompleted: Type.apply(this, Type.bodyUI)
                    }
                }
            }
        }

        Item { Layout.fillHeight: true }

        StatusPill {
            Layout.alignment: Qt.AlignHCenter
            visible: ResponsiveStore.expanded
            text: qsTr("Local-only")
            subtype: "Network-Offline"
            iconSource: "qrc:/icons/lock.svg"
        }

        Text {
            Layout.fillWidth: true
            visible: ResponsiveStore.expanded && StorageStore.warning
            text: qsTr("Storage %1% full").arg(Math.round(StorageStore.usageRatio * 100))
            color: StorageStore.critical ? Theme.error : Theme.warning
            wrapMode: Text.Wrap
            horizontalAlignment: Text.AlignHCenter
            Component.onCompleted: Type.apply(this, Type.caption)
        }
    }
}
""",
    )


def write_left_drawer() -> None:
    write(
        ROOT / "qml/components/LeftDrawer.qml",
        """import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../architecture"
import "../stores"
import "../theme"

Drawer {
    id: root

    width: Math.min(parent ? parent.width * 0.88 : 360, 408)
    height: parent ? parent.height : implicitHeight
    edge: Qt.LeftEdge
    modal: true
    interactive: true

    background: Rectangle {
        color: Theme.p.surface
        border.width: Theme.highContrast ? 1 : 0
        border.color: Theme.p.divider
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.leftMargin: Spacing.lg
        anchors.rightMargin: Spacing.lg
        anchors.topMargin: Spacing.lg
        anchors.bottomMargin: Spacing.lg
        spacing: Spacing.md

        RowLayout {
            Layout.fillWidth: true
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 0
                Text {
                    text: qsTr("Mukei")
                    color: Theme.p.inkPrimary
                    Component.onCompleted: Type.apply(this, Type.h2)
                }
                Text {
                    text: qsTr("Private, on this device")
                    color: Theme.p.inkSecondary
                    Component.onCompleted: Type.apply(this, Type.caption)
                }
            }
            IconButton {
                iconSource: "qrc:/icons/close.svg"
                text: qsTr("Close drawer")
                onClicked: root.close()
            }
        }

        PrimaryButton {
            Layout.fillWidth: true
            text: qsTr("New conversation")
            enabled: CapabilityStore.canClearConversation || CapabilityStore.canSendMessage
            onClicked: {
                if (CapabilityStore.canClearConversation)
                    IntentDispatcher.dispatch({ type: "chat.clearConversation" })
                IntentDispatcher.dispatch({ type: "navigation.open", route: "chat" })
                root.close()
            }
        }

        SearchField { Layout.fillWidth: true }

        ConversationList {
            Layout.fillWidth: true
            Layout.fillHeight: true
            onConversationSelected: function(conversationId, branchId) {
                IntentDispatcher.dispatch({
                    type: "conversation.open",
                    conversationId: conversationId,
                    branchId: branchId
                })
                root.close()
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: Spacing.xs

            GhostButton {
                Layout.fillWidth: true
                text: qsTr("Models")
                onClicked: {
                    IntentDispatcher.dispatch({ type: "navigation.open", route: "models" })
                    root.close()
                }
            }
            GhostButton {
                Layout.fillWidth: true
                text: qsTr("Settings")
                onClicked: {
                    IntentDispatcher.dispatch({ type: "navigation.open", route: "settings" })
                    root.close()
                }
            }
        }

        StatusPill {
            Layout.alignment: Qt.AlignHCenter
            text: qsTr("Local-only")
            subtype: "Network-Offline"
            iconSource: "qrc:/icons/lock.svg"
        }
    }
}
""",
    )


def write_chat_composer() -> None:
    write(
        ROOT / "qml/components/ChatComposer.qml",
        """import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

FocusScope {
    id: root

    property alias text: textArea.text
    property alias cursorPosition: textArea.cursorPosition
    property bool isStreaming: false
    property bool canSend: true
    signal sendRequested(string text)
    signal stopRequested
    signal attachRequested

    function forceEditorFocus() {
        textArea.forceActiveFocus()
    }

    Accessible.role: Accessible.EditableText
    Accessible.name: qsTr("Compose message")
    Accessible.description: qsTr("Type a private message to Mukei")
    implicitHeight: Math.max(56,
                             Math.min(textArea.contentHeight + Spacing.md * 2,
                                      Type.bodyUI.pixelSize * 6 + Spacing.lg))

    Rectangle {
        anchors.fill: parent
        radius: Theme.radiusXl
        color: Theme.p.surface
        border.width: textArea.activeFocus || Theme.highContrast ? 2 : 1
        border.color: textArea.activeFocus ? Theme.p.accent : Theme.p.divider

        Behavior on border.color {
            ColorAnimation { duration: Theme.reduceMotion ? 0 : Motion.microTransition }
        }
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: Spacing.sm
        anchors.rightMargin: Spacing.sm
        anchors.topMargin: Spacing.xs
        anchors.bottomMargin: Spacing.xs
        spacing: Spacing.xs

        IconButton {
            iconSource: "qrc:/icons/attach.svg"
            text: qsTr("Attach local file")
            Accessible.description: qsTr("Add a private local document to this conversation")
            onClicked: root.attachRequested()
        }

        TextArea {
            id: textArea
            Layout.fillWidth: true
            Layout.minimumHeight: Type.bodyUI.pixelSize
            Layout.maximumHeight: Type.bodyUI.pixelSize * 6
            wrapMode: TextArea.Wrap
            color: Theme.p.inkPrimary
            selectionColor: Qt.rgba(Theme.p.accent.r, Theme.p.accent.g, Theme.p.accent.b, 0.22)
            selectedTextColor: Theme.p.inkPrimary
            placeholderText: qsTr("Ask Mukei anything…")
            placeholderTextColor: Theme.p.inkFaint
            background: null
            Accessible.name: qsTr("Message text")
            Accessible.description: qsTr("One to six line message editor")
            Component.onCompleted: Type.apply(this, Type.bodyUI)

            Keys.onPressed: function(event) {
                if ((event.modifiers & (Qt.ControlModifier | Qt.MetaModifier))
                        && event.key === Qt.Key_Return) {
                    if (!root.isStreaming && root.canSend && textArea.text.trim().length > 0)
                        root.sendRequested(textArea.text)
                    event.accepted = true
                }
            }
        }

        Button {
            id: sendButton
            implicitWidth: Spacing.xxl
            implicitHeight: Spacing.xxl
            enabled: root.isStreaming || (root.canSend && textArea.text.trim().length > 0)
            Accessible.name: root.isStreaming ? qsTr("Stop response") : qsTr("Send message")
            onClicked: root.isStreaming ? root.stopRequested() : root.sendRequested(textArea.text)

            background: Rectangle {
                radius: width / 2
                color: sendButton.enabled ? Theme.p.accent : Theme.p.surfaceVariant
                scale: sendButton.down && !Theme.reduceMotion ? 0.94 : 1
                Behavior on scale {
                    NumberAnimation { duration: Motion.immediateFeedback; easing.type: Easing.OutCubic }
                }
            }

            contentItem: MukeiIcon {
                source: root.isStreaming ? "qrc:/icons/stop.svg" : "qrc:/icons/send.svg"
                color: sendButton.enabled ? Theme.p.background : Theme.p.inkFaint
            }
        }
    }
}
""",
    )


def write_empty_chat() -> None:
    write(
        ROOT / "qml/screens/EmptyChatScreen.qml",
        """import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"
import "../components"

Item {
    id: root

    signal promptFilled(string prompt)

    Accessible.role: Accessible.Pane
    Accessible.name: qsTr("Empty chat")
    Accessible.description: qsTr("Start a private on-device conversation")

    ColumnLayout {
        anchors.centerIn: parent
        width: Math.min(parent.width, 640)
        spacing: Spacing.md

        Text {
            Layout.fillWidth: true
            text: qsTr("Mukei is ready.")
            color: Theme.p.inkPrimary
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.display)
        }

        Text {
            Layout.fillWidth: true
            text: qsTr("Everything runs on your device.")
            color: Theme.p.inkSecondary
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.bodyUI)
        }

        StatusPill {
            Layout.alignment: Qt.AlignHCenter
            text: qsTr("Encrypted locally")
            subtype: "Network-Offline"
            iconSource: "qrc:/icons/lock.svg"
        }

        Item { Layout.preferredHeight: Spacing.lg }

        Text {
            Layout.fillWidth: true
            text: qsTr("Try one of these to start")
            color: Theme.p.inkSecondary
            horizontalAlignment: Text.AlignHCenter
            Component.onCompleted: Type.apply(this, Type.caption)
        }

        PromptCard {
            Layout.fillWidth: true
            prompt: qsTr("Summarize the concept of entropy.")
            onFillRequested: root.promptFilled(prompt)
        }
        PromptCard {
            Layout.fillWidth: true
            prompt: qsTr("Draft a privacy-first project plan.")
            onFillRequested: root.promptFilled(prompt)
        }
        PromptCard {
            Layout.fillWidth: true
            prompt: qsTr("Explain this note in plain language.")
            onFillRequested: root.promptFilled(prompt)
        }
    }
}
""",
    )


def write_chat_screen() -> None:
    write(
        ROOT / "qml/screens/ChatScreen.qml",
        """import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtQml.Models
import Qt.labs.qmlmodels
import "../architecture"
import "../stores"
import "../theme"
import "../components"

Page {
    id: root

    property bool followTail: true
    property bool unseenTailUpdate: false
    property string queuedPrompt: ""
    signal accessibilityAnnouncementRequested(string text)

    background: Rectangle { color: Theme.p.background }
    Accessible.role: Accessible.Pane
    Accessible.name: qsTr("Chat")

    Keys.onEscapePressed: {
        if (ChatStore.streaming)
            IntentDispatcher.dispatch({ type: "chat.stopGeneration" })
    }

    Timer {
        id: promptSendTimer
        interval: 600
        repeat: false
        onTriggered: {
            if (root.queuedPrompt.length > 0 && composer.text === root.queuedPrompt
                    && composer.canSend && !composer.isStreaming) {
                IntentDispatcher.dispatch({ type: "chat.sendMessage", text: root.queuedPrompt })
                root.queuedPrompt = ""
            }
        }
    }

    LeftDrawer { id: drawer }

    RowLayout {
        anchors.fill: parent
        spacing: 0

        Rectangle {
            Layout.preferredWidth: 320
            Layout.fillHeight: true
            visible: ResponsiveStore.expanded
            color: Theme.p.surface

            ColumnLayout {
                anchors.fill: parent
                anchors.margins: Spacing.lg
                spacing: Spacing.md

                RowLayout {
                    Layout.fillWidth: true
                    Text {
                        Layout.fillWidth: true
                        text: qsTr("Conversations")
                        color: Theme.p.inkPrimary
                        Component.onCompleted: Type.apply(this, Type.h3)
                    }
                    IconButton {
                        iconSource: "qrc:/icons/chat.svg"
                        text: qsTr("New conversation")
                        enabled: CapabilityStore.canClearConversation || CapabilityStore.canSendMessage
                        onClicked: IntentDispatcher.dispatch({ type: "chat.clearConversation" })
                    }
                }

                SearchField { Layout.fillWidth: true }

                ConversationList {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    onConversationSelected: function(conversationId, branchId) {
                        IntentDispatcher.dispatch({
                            type: "conversation.open",
                            conversationId: conversationId,
                            branchId: branchId
                        })
                    }
                }
            }
        }

        ColumnLayout {
            id: chatPane
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.leftMargin: ResponsiveStore.edgePadding
            Layout.rightMargin: ResponsiveStore.edgePadding
            Layout.topMargin: Spacing.md
            Layout.bottomMargin: Spacing.md
            spacing: Spacing.sm

            RowLayout {
                Layout.fillWidth: true
                Layout.maximumWidth: ResponsiveStore.contentMaxWidth
                Layout.alignment: Qt.AlignHCenter
                spacing: Spacing.xs

                IconButton {
                    visible: !ResponsiveStore.expanded
                    iconSource: "qrc:/icons/chat.svg"
                    text: qsTr("Open conversations")
                    onClicked: drawer.open()
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0
                    Text {
                        Layout.fillWidth: true
                        text: qsTr("Mukei")
                        color: Theme.p.inkPrimary
                        Component.onCompleted: Type.apply(this, Type.h3)
                    }
                    Text {
                        Layout.fillWidth: true
                        text: ChatStore.streaming
                              ? qsTr("Responding privately on device")
                              : qsTr("Private local conversation")
                        color: Theme.p.inkSecondary
                        elide: Text.ElideRight
                        Component.onCompleted: Type.apply(this, Type.caption)
                    }
                }

                StatusPill {
                    visible: !Type.compact
                    text: qsTr("Local-only")
                    subtype: "Network-Offline"
                    iconSource: "qrc:/icons/lock.svg"
                }

                IconButton {
                    iconSource: "qrc:/icons/settings.svg"
                    text: qsTr("Open settings")
                    enabled: CapabilityStore.canOpenSettings
                    onClicked: IntentDispatcher.dispatch({ type: "navigation.open", route: "settings" })
                }
            }

            Item {
                Layout.fillWidth: true
                Layout.fillHeight: true
                Layout.maximumWidth: ResponsiveStore.contentMaxWidth
                Layout.alignment: Qt.AlignHCenter

                ListView {
                    id: timelineView
                    objectName: "chatTimelineView"
                    property real contentHeightBeforePrepend: -1
                    anchors.fill: parent
                    clip: true
                    spacing: Spacing.lg
                    model: ChatStore.timeline
                    cacheBuffer: Math.max(height, Spacing.huge * 6)
                    boundsBehavior: Flickable.StopAtBounds
                    reuseItems: true

                    header: Item {
                        width: ListView.view ? ListView.view.width : 0
                        height: ChatStore.hasOlderMessages
                                ? loadOlderButton.implicitHeight + Spacing.md
                                : 0
                        visible: ChatStore.hasOlderMessages
                        GhostButton {
                            id: loadOlderButton
                            anchors.horizontalCenter: parent.horizontalCenter
                            text: ChatStore.olderPageLoading ? qsTr("Loading…") : qsTr("Load earlier messages")
                            enabled: !ChatStore.olderPageLoading
                            onClicked: {
                                if (ListView.view)
                                    ListView.view.contentHeightBeforePrepend = ListView.view.contentHeight
                                IntentDispatcher.dispatch({ type: "chat.loadOlder" })
                            }
                        }
                    }

                    onMovementEnded: {
                        root.followTail = atYEnd
                        if (root.followTail)
                            root.unseenTailUpdate = false
                    }
                    onCountChanged: {
                        if (root.followTail)
                            Qt.callLater(positionViewAtEnd)
                        else
                            root.unseenTailUpdate = true
                    }

                    delegate: DelegateChooser {
                        role: "type"
                        DelegateChoice {
                            roleValue: "user_message"
                            delegate: UserMessageBubble {
                                id: userMessageDelegate
                                required property var model
                                width: ListView.view ? ListView.view.width : 0
                                text: userMessageDelegate.model.text
                                timestamp: userMessageDelegate.model.timestamp
                            }
                        }
                        DelegateChoice {
                            roleValue: "assistant_message"
                            delegate: AIMessageBubble {
                                id: assistantMessageDelegate
                                required property var model
                                width: ListView.view ? ListView.view.width : 0
                                text: assistantMessageDelegate.model.text
                                timestamp: assistantMessageDelegate.model.timestamp
                            }
                        }
                        DelegateChoice {
                            roleValue: "timeline_event"
                            delegate: ChatTimelineEvent {
                                id: timelineEventDelegate
                                required property var model
                                width: ListView.view ? ListView.view.width : 0
                                label: timelineEventDelegate.model.text
                                phase: timelineEventDelegate.model.phase
                                kind: timelineEventDelegate.model.kind
                                iconSource: timelineEventDelegate.model.kind === "tool"
                                            ? "qrc:/icons/search.svg" : ""
                            }
                        }
                    }
                }

                EmptyChatScreen {
                    anchors.fill: parent
                    visible: timelineView.count === 0 && !ChatStore.snapshotLoading
                    onPromptFilled: function(prompt) {
                        root.queuedPrompt = prompt
                        composer.text = prompt
                        composer.cursorPosition = prompt.length
                        composer.forceEditorFocus()
                        promptSendTimer.restart()
                    }
                }

                BusyIndicator {
                    anchors.centerIn: parent
                    visible: ChatStore.snapshotLoading
                    running: visible
                }
            }

            GhostButton {
                Layout.alignment: Qt.AlignHCenter
                visible: root.unseenTailUpdate
                text: qsTr("↓ Latest")
                onClicked: {
                    root.followTail = true
                    root.unseenTailUpdate = false
                    timelineView.positionViewAtEnd()
                }
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.maximumWidth: ResponsiveStore.contentMaxWidth
                Layout.alignment: Qt.AlignHCenter
                visible: !CapabilityStore.activeModelReady
                implicitHeight: modelNoticeRow.implicitHeight + Spacing.md * 2
                radius: Theme.radiusLg
                color: Theme.p.surfaceFaint

                RowLayout {
                    id: modelNoticeRow
                    anchors.fill: parent
                    anchors.margins: Spacing.md
                    spacing: Spacing.sm

                    Text {
                        Layout.fillWidth: true
                        text: qsTr("Choose a verified local model to begin.")
                        color: Theme.p.inkSecondary
                        wrapMode: Text.Wrap
                        Component.onCompleted: Type.apply(this, Type.bodySmall)
                    }
                    GhostButton {
                        text: qsTr("Models")
                        onClicked: IntentDispatcher.dispatch({ type: "navigation.open", route: "models" })
                    }
                }
            }

            ChatComposer {
                id: composer
                Layout.fillWidth: true
                Layout.maximumWidth: ResponsiveStore.contentMaxWidth
                Layout.alignment: Qt.AlignHCenter
                isStreaming: ChatStore.streaming
                canSend: CapabilityStore.canSendMessage && !ChatStore.streaming
                text: ChatStore.draft
                cursorPosition: Math.min(ChatStore.draftCursorPosition, text.length)

                onTextChanged: {
                    if (text !== root.queuedPrompt)
                        promptSendTimer.stop()
                    if (text !== ChatStore.draft)
                        IntentDispatcher.dispatch({
                            type: "chat.updateDraft",
                            text: text,
                            cursorPosition: cursorPosition
                        })
                }
                onCursorPositionChanged: {
                    if (text === ChatStore.draft && cursorPosition !== ChatStore.draftCursorPosition)
                        IntentDispatcher.dispatch({
                            type: "chat.updateDraft",
                            text: text,
                            cursorPosition: cursorPosition
                        })
                }
                onSendRequested: function(message) {
                    root.queuedPrompt = ""
                    promptSendTimer.stop()
                    IntentDispatcher.dispatch({ type: "chat.sendMessage", text: message })
                }
                onStopRequested: IntentDispatcher.dispatch({ type: "chat.stopGeneration" })
            }

            NetworkBanner {
                Layout.fillWidth: true
                Layout.maximumWidth: ResponsiveStore.contentMaxWidth
                Layout.alignment: Qt.AlignHCenter
                remoteAllowed: SettingsStore.remotePolicy === "remote_allowed"
            }
        }
    }

    Connections {
        target: ChatStore
        function onTailUpdated() {
            if (root.followTail)
                Qt.callLater(timelineView.positionViewAtEnd)
            else
                root.unseenTailUpdate = true
        }
        function onDraftChangedByStore(value, cursorPosition) {
            if (composer.text !== value)
                composer.text = value
            composer.cursorPosition = Math.min(cursorPosition, composer.text.length)
        }
        function onSnapshotApplied() {
            if (timelineView.contentHeightBeforePrepend >= 0) {
                var delta = timelineView.contentHeight - timelineView.contentHeightBeforePrepend
                timelineView.contentY += Math.max(0, delta)
                timelineView.contentHeightBeforePrepend = -1
            } else if (root.followTail) {
                Qt.callLater(timelineView.positionViewAtEnd)
            }
        }
    }

    Component.onCompleted: {
        if (NavigationStore.currentParameters.conversationId
                && NavigationStore.currentParameters.branchId
                && (ChatStore.activeConversationId !== NavigationStore.currentParameters.conversationId
                    || ChatStore.activeBranchId !== NavigationStore.currentParameters.branchId)) {
            ChatStore.openConversation(
                        NavigationStore.currentParameters.conversationId,
                        NavigationStore.currentParameters.branchId)
        } else {
            ChatStore.restoreDraft()
        }
    }
}
""",
    )


def write_settings_screen() -> None:
    write(
        ROOT / "qml/screens/SettingsScreen.qml",
        """pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../architecture"
import "../stores"
import "../theme"
import "../components"

Page {
    id: root

    property int selectedTab: 0
    readonly property var tabs: [qsTr("General"), qsTr("Privacy"), qsTr("Storage"), qsTr("About")]

    background: Rectangle { color: Theme.p.background }
    Accessible.role: Accessible.Pane
    Accessible.name: qsTr("Settings")

    ColumnLayout {
        anchors.fill: parent
        anchors.leftMargin: ResponsiveStore.edgePadding
        anchors.rightMargin: ResponsiveStore.edgePadding
        anchors.topMargin: Spacing.md
        anchors.bottomMargin: Spacing.md
        spacing: Spacing.lg

        RowLayout {
            Layout.fillWidth: true
            Layout.maximumWidth: ResponsiveStore.contentMaxWidth
            Layout.alignment: Qt.AlignHCenter

            IconButton {
                visible: ResponsiveStore.compact
                iconSource: "qrc:/icons/back.svg"
                text: qsTr("Back")
                onClicked: IntentDispatcher.dispatch({ type: "navigation.back" })
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 0
                Text {
                    text: qsTr("Settings")
                    color: Theme.p.inkPrimary
                    Component.onCompleted: Type.apply(this, Type.h1)
                }
                Text {
                    text: qsTr("Private preferences stored on this device")
                    color: Theme.p.inkSecondary
                    Component.onCompleted: Type.apply(this, Type.caption)
                }
            }
        }

        ScrollView {
            Layout.fillWidth: true
            Layout.maximumWidth: ResponsiveStore.contentMaxWidth
            Layout.alignment: Qt.AlignHCenter
            implicitHeight: 52
            contentWidth: tabsRow.implicitWidth
            ScrollBar.vertical.policy: ScrollBar.AlwaysOff
            ScrollBar.horizontal.policy: ScrollBar.AsNeeded

            RowLayout {
                id: tabsRow
                spacing: Spacing.xs

                Repeater {
                    model: root.tabs
                    delegate: Button {
                        id: tabButton
                        required property var modelData
                        required property int index
                        implicitHeight: 48
                        text: tabButton.modelData
                        checked: root.selectedTab === tabButton.index
                        checkable: true
                        onClicked: root.selectedTab = tabButton.index

                        background: Item {
                            Rectangle {
                                visible: tabButton.checked
                                height: 3
                                radius: 2
                                anchors.left: parent.left
                                anchors.right: parent.right
                                anchors.bottom: parent.bottom
                                color: Theme.p.accent
                            }
                        }

                        contentItem: Text {
                            text: tabButton.text
                            color: tabButton.checked ? Theme.p.inkPrimary : Theme.p.inkSecondary
                            horizontalAlignment: Text.AlignHCenter
                            verticalAlignment: Text.AlignVCenter
                            Component.onCompleted: Type.apply(this, Type.h3)
                        }
                    }
                }
            }
        }

        StackLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.maximumWidth: ResponsiveStore.contentMaxWidth
            Layout.alignment: Qt.AlignHCenter
            currentIndex: root.selectedTab

            ScrollView {
                clip: true
                ColumnLayout {
                    width: Math.min(parent.width, 760)
                    spacing: Spacing.lg

                    Text { text: qsTr("Appearance"); color: Theme.p.inkPrimary; Component.onCompleted: Type.apply(this, Type.h2) }

                    Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: appearanceColumn.implicitHeight + Spacing.lg * 2
                        radius: Theme.radiusXl
                        color: Theme.p.surface

                        ColumnLayout {
                            id: appearanceColumn
                            anchors.fill: parent
                            anchors.margins: Spacing.lg
                            spacing: Spacing.md

                            Text { text: qsTr("Theme"); color: Theme.p.inkSecondary; Component.onCompleted: Type.apply(this, Type.caption) }

                            GridLayout {
                                Layout.fillWidth: true
                                columns: ResponsiveStore.compact ? 1 : 3
                                columnSpacing: Spacing.xs
                                rowSpacing: Spacing.xs

                                SecondaryButton { Layout.fillWidth: true; text: qsTr("Dolce Vita"); onClicked: IntentDispatcher.dispatch({ type: "settings.update", key: "theme_mode", value: "dolce_vita" }) }
                                SecondaryButton { Layout.fillWidth: true; text: qsTr("Espresso"); onClicked: IntentDispatcher.dispatch({ type: "settings.update", key: "theme_mode", value: "espresso" }) }
                                SecondaryButton { Layout.fillWidth: true; text: qsTr("Taupe"); onClicked: IntentDispatcher.dispatch({ type: "settings.update", key: "theme_mode", value: "taupe" }) }
                            }

                            CheckBox { text: qsTr("Reduce motion"); checked: SettingsStore.reduceMotion; onToggled: IntentDispatcher.dispatch({ type: "settings.update", key: "reduce_motion", value: checked }) }
                            CheckBox { text: qsTr("High contrast"); checked: SettingsStore.highContrast; onToggled: IntentDispatcher.dispatch({ type: "settings.update", key: "high_contrast", value: checked }) }

                            Text { text: qsTr("Text size · %1%").arg(Math.round(SettingsStore.fontScalePercent)); color: Theme.p.inkSecondary; Component.onCompleted: Type.apply(this, Type.bodySmall) }
                            Slider {
                                Layout.fillWidth: true
                                from: 85
                                to: 200
                                stepSize: 5
                                value: SettingsStore.fontScalePercent
                                onMoved: Theme.scale = value / 100
                                onPressedChanged: if (!pressed) IntentDispatcher.dispatch({ type: "settings.update", key: "font_scale_percent", value: Math.round(value) })
                                Accessible.name: qsTr("Text size percentage")
                            }
                        }
                    }

                    Text { text: qsTr("Inference defaults"); color: Theme.p.inkPrimary; Component.onCompleted: Type.apply(this, Type.h2) }

                    Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: inferenceColumn.implicitHeight + Spacing.lg * 2
                        radius: Theme.radiusXl
                        color: Theme.p.surface

                        ColumnLayout {
                            id: inferenceColumn
                            anchors.fill: parent
                            anchors.margins: Spacing.lg
                            spacing: Spacing.md

                            Text {
                                Layout.fillWidth: true
                                text: qsTr("Changes apply to the next message and never alter an active response.")
                                color: Theme.p.inkSecondary
                                wrapMode: Text.Wrap
                                Component.onCompleted: Type.apply(this, Type.bodySmall)
                            }

                            SettingsTextField {
                                Layout.fillWidth: true
                                label: qsTr("Temperature")
                                text: (SettingsStore.temperatureMilli / 1000).toFixed(2)
                                inputMethodHints: Qt.ImhFormattedNumbersOnly
                                onEditingFinished: function(value) {
                                    var parsed = Math.max(0, Math.min(2000, Math.round(Number(value) * 1000)))
                                    IntentDispatcher.dispatch({ type: "settings.update", key: "temperature_milli", value: parsed })
                                }
                            }
                            SettingsTextField {
                                Layout.fillWidth: true
                                label: qsTr("Max tokens")
                                text: String(SettingsStore.maxTokens)
                                inputMethodHints: Qt.ImhDigitsOnly
                                onEditingFinished: function(value) {
                                    IntentDispatcher.dispatch({ type: "settings.update", key: "max_tokens_default", value: Math.max(64, Math.min(32768, Number(value))) })
                                }
                            }
                            SettingsTextField {
                                Layout.fillWidth: true
                                label: qsTr("Top-p")
                                text: (SettingsStore.topPMilli / 1000).toFixed(2)
                                inputMethodHints: Qt.ImhFormattedNumbersOnly
                                onEditingFinished: function(value) {
                                    var parsed = Math.max(1, Math.min(1000, Math.round(Number(value) * 1000)))
                                    IntentDispatcher.dispatch({ type: "settings.update", key: "top_p_milli", value: parsed })
                                }
                            }
                        }
                    }
                    Item { Layout.preferredHeight: Spacing.xl }
                }
            }

            ScrollView {
                clip: true
                ColumnLayout {
                    width: Math.min(parent.width, 760)
                    spacing: Spacing.lg

                    Text { text: qsTr("Privacy"); color: Theme.p.inkPrimary; Component.onCompleted: Type.apply(this, Type.h2) }

                    Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: privacyColumn.implicitHeight + Spacing.lg * 2
                        radius: Theme.radiusXl
                        color: Theme.p.surface

                        ColumnLayout {
                            id: privacyColumn
                            anchors.fill: parent
                            anchors.margins: Spacing.lg
                            spacing: Spacing.md

                            Text { Layout.fillWidth: true; text: qsTr("All data lives on this device."); color: Theme.p.inkPrimary; wrapMode: Text.Wrap; Component.onCompleted: Type.apply(this, Type.h3) }
                            Text { Layout.fillWidth: true; text: qsTr("No accounts. No telemetry. No background sync. Remote tools stay blocked unless you explicitly allow them."); color: Theme.p.inkSecondary; wrapMode: Text.Wrap; Component.onCompleted: Type.apply(this, Type.bodyUI) }
                            StatusPill { text: qsTr("Encrypted local database"); subtype: "Success"; iconSource: "qrc:/icons/lock.svg" }
                            StatusPill { text: qsTr("Crash logs stay on device"); subtype: "Success"; iconSource: "qrc:/icons/check.svg" }
                            StatusPill { text: qsTr("No account required"); subtype: "Success"; iconSource: "qrc:/icons/check.svg" }
                        }
                    }

                    Text { text: qsTr("Remote tools"); color: Theme.p.inkPrimary; Component.onCompleted: Type.apply(this, Type.h2) }
                    RadioButton { text: qsTr("Local only"); checked: SettingsStore.remotePolicy === "local_only"; onClicked: IntentDispatcher.dispatch({ type: "settings.update", key: "remote_feature_policy", value: "local_only" }) }
                    RadioButton { text: qsTr("Ask before remote access"); checked: SettingsStore.remotePolicy === "ask_before_remote"; onClicked: IntentDispatcher.dispatch({ type: "settings.update", key: "remote_feature_policy", value: "ask_before_remote" }) }
                    RadioButton { text: qsTr("Allow remote tools"); checked: SettingsStore.remotePolicy === "remote_allowed"; onClicked: IntentDispatcher.dispatch({ type: "settings.update", key: "remote_feature_policy", value: "remote_allowed" }) }
                    GhostButton { text: qsTr("Manage private documents"); onClicked: IntentDispatcher.dispatch({ type: "navigation.open", route: "documents" }) }
                    Item { Layout.preferredHeight: Spacing.xl }
                }
            }

            ScrollView {
                clip: true
                ColumnLayout {
                    width: Math.min(parent.width, 760)
                    spacing: Spacing.lg
                    Text { text: qsTr("Storage"); color: Theme.p.inkPrimary; Component.onCompleted: Type.apply(this, Type.h2) }
                    StoragePressureCard { Layout.fillWidth: true }

                    Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: storageColumn.implicitHeight + Spacing.lg * 2
                        radius: Theme.radiusXl
                        color: Theme.p.surface

                        ColumnLayout {
                            id: storageColumn
                            anchors.fill: parent
                            anchors.margins: Spacing.lg
                            spacing: Spacing.md
                            Text { text: qsTr("Verified models · %1").arg(StorageStore.formatBytes(StorageStore.modelBytes)); color: Theme.p.inkSecondary; Component.onCompleted: Type.apply(this, Type.bodyUI) }
                            Text { text: qsTr("Partial downloads · %1").arg(StorageStore.formatBytes(StorageStore.partialBytes)); color: Theme.p.inkSecondary; Component.onCompleted: Type.apply(this, Type.bodyUI) }
                            RowLayout {
                                Layout.fillWidth: true
                                SecondaryButton { Layout.fillWidth: true; text: qsTr("Manage models"); onClicked: IntentDispatcher.dispatch({ type: "navigation.open", route: "models" }) }
                                GhostButton { Layout.fillWidth: true; text: qsTr("Downloads"); onClicked: IntentDispatcher.dispatch({ type: "navigation.open", route: "downloads" }) }
                            }
                        }
                    }
                    GhostButton { text: qsTr("Refresh storage"); onClicked: IntentDispatcher.dispatch({ type: "storage.refresh" }) }
                    Item { Layout.preferredHeight: Spacing.xl }
                }
            }

            ScrollView {
                clip: true
                ColumnLayout {
                    width: Math.min(parent.width, 760)
                    spacing: Spacing.lg
                    Text { text: qsTr("Mukei"); color: Theme.p.inkPrimary; Component.onCompleted: Type.apply(this, Type.display) }
                    Text { text: qsTr("Version 0.7.5 · Local-first AI assistant"); color: Theme.p.inkSecondary; Component.onCompleted: Type.apply(this, Type.bodyUI) }

                    Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: aboutColumn.implicitHeight + Spacing.lg * 2
                        radius: Theme.radiusXl
                        color: Theme.p.surface
                        ColumnLayout {
                            id: aboutColumn
                            anchors.fill: parent
                            anchors.margins: Spacing.lg
                            spacing: Spacing.md
                            Text { Layout.fillWidth: true; text: qsTr("Calm. Capable. Confidential. Crafted."); color: Theme.p.inkPrimary; wrapMode: Text.Wrap; Component.onCompleted: Type.apply(this, Type.h3) }
                            Text { Layout.fillWidth: true; text: qsTr("Licensed under Apache License 2.0. Third-party components retain their own licenses."); color: Theme.p.inkSecondary; wrapMode: Text.Wrap; Component.onCompleted: Type.apply(this, Type.bodyUI) }
                            StatusPill { text: qsTr("On-device runtime"); subtype: "Success"; iconSource: "qrc:/icons/lock.svg" }
                        }
                    }
                    GhostButton { text: qsTr("Diagnostics"); onClicked: IntentDispatcher.dispatch({ type: "navigation.open", route: "diagnostics" }) }
                    Item { Layout.preferredHeight: Spacing.xl }
                }
            }
        }
    }

    Component.onCompleted: {
        SettingsStore.hydrate()
        StorageStore.hydrate()
    }
}
""",
    )


def patch_workflow() -> None:
    path = ROOT / ".github/workflows/android-apk-build.yml"
    text = path.read_text(encoding="utf-8")
    text = text.replace(
        """on:
  workflow_dispatch:
  pull_request:
    branches: [ "checkpoint/stabilization-2026-07-12" ]
""",
        """on:
  workflow_dispatch:
  push:
    branches:
      - "agent/full-qml-fixed-build-2026-07-15"
    paths:
      - ".github/workflows/android-apk-build.yml"
      - "qml/**"
      - "rust/**"
      - "scripts/android/**"
  pull_request:
    branches: [ "main" ]
""",
        1,
    )
    path.write_text(text, encoding="utf-8")


def main() -> int:
    patch_cmake()
    patch_build_script()
    patch_main_cpp()
    patch_event_dispatcher()
    patch_intent_dispatcher()
    patch_responsive_store()
    write_main_window()
    write_app_shell()
    write_adaptive_navigation()
    write_left_drawer()
    write_chat_composer()
    write_empty_chat()
    write_chat_screen()
    write_settings_screen()
    patch_workflow()
    print("Production frontend migration applied")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
