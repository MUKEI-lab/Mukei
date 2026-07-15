import QtQuick
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
