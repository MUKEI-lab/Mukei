import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
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

    Behavior on color {
        enabled: !Theme.reduceMotion
        ColorAnimation { duration: Motion.themeCrossFade; easing.type: Easing.OutCubic }
    }

    LayoutMirroring.enabled: Qt.application.layoutDirection === Qt.RightToLeft
    LayoutMirroring.childrenInherit: true

    signal accessibilityAnnouncementRequested(string text)

    AppShell {
        anchors.fill: parent
    }

    onWidthChanged: ResponsiveStore.updateViewport(width, height)
    onHeightChanged: ResponsiveStore.updateViewport(width, height)

    Component.onCompleted: {
        ResponsiveStore.updateViewport(width, height)
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
        function onStateChanged() {
            AppCoordinator.onApplicationStateChanged(Qt.application.state)
        }
    }

    Shortcut {
        sequence: StandardKey.Preferences
        enabled: CapabilityStore.canOpenSettings
        onActivated: IntentDispatcher.dispatch({
            type: "navigation.open",
            route: "settings"
        })
    }

    Shortcut {
        sequence: StandardKey.Back
        onActivated: IntentDispatcher.dispatch({ type: "navigation.back" })
    }
}
