import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Effects
import "../theme"

Button {
    id: root
    property string iconSource: ""
    property bool active: false
    Accessible.role: Accessible.Button
    Accessible.name: text.length > 0 ? text : qsTr("Icon action")
    activeFocusOnTab: true
    implicitWidth: Math.max(44, Spacing.xxl)
    implicitHeight: Math.max(44, Spacing.xxl)
    padding: Spacing.sm

    background: Rectangle {
        radius: Theme.radiusXxl
        color: root.down || root.hovered || root.active ? Theme.p.surfaceFaint : "transparent"
        border.width: root.visualFocus || Theme.highContrast ? 1 : 0
        border.color: root.active ? Theme.p.accent : Theme.p.divider
        scale: root.down && !Theme.reduceMotion ? 0.96 : 1
        Behavior on scale { NumberAnimation { duration: Motion.immediateFeedback; easing.type: Easing.OutCubic } }
    }

    contentItem: Item {
        Image {
            id: iconImage
            anchors.centerIn: parent
            width: Spacing.lg
            height: Spacing.lg
            source: root.iconSource
            sourceSize: Qt.size(Spacing.lg, Spacing.lg)
            fillMode: Image.PreserveAspectFit
            visible: false
        }
        MultiEffect {
            anchors.fill: iconImage
            source: iconImage
            colorization: 1
            colorizationColor: root.active ? Theme.p.accent : Theme.p.inkPrimary
            opacity: root.enabled ? 1 : 0.38
        }
    }
}
