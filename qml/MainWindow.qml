import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "theme"
import "components"
import "screens"

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

    StackView {
        id: stack
        anchors.fill: parent
        initialItem: ChatScreen {
        }
    }

    Shortcut {
        sequence: StandardKey.Preferences
        onActivated: stack.push(settingsComponent)
    }

    Component {
        id: settingsComponent
        SettingsScreen {
        }
    }
}
