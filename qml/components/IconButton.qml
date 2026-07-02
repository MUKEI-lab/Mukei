import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

Control {
    id: root

    property string iconSource: ""
    property bool active: false
    signal clicked

    Accessible.role: Accessible.Button
    Accessible.name: qsTr("Activate icon")
    Accessible.description: qsTr("Icon-only action button")
    implicitWidth: Spacing.xxl
    implicitHeight: Spacing.xxl

    background: Rectangle {
        radius: Spacing.lg
        color: root.hovered || root.activeFocus ? Theme.p.surfaceFaint : "transparent"
        border.width: Theme.highContrast || root.activeFocus ? 1 : 0
        border.color: root.active ? Theme.p.accent : Theme.p.divider
    }

    contentItem: Image {
        source: root.iconSource
        sourceSize.width: Spacing.lg
        sourceSize.height: Spacing.lg
        fillMode: Image.PreserveAspectFit
        opacity: root.enabled ? 1 : 0.4
        // Apply colorization effect to tint icons per theme and active state
        // Uses MultiEffect for runtime color overlay bound to theme colors
        layer.enabled: true
        layer.effect: MultiEffect {
            id: iconEffect
            colorization: root.active ? Theme.p.accent : (root.enabled ? Theme.p.inkPrimary : Theme.p.inkFaint)
            colorizationAmount: 1.0
        }
    }

    TapHandler {
        onTapped: root.clicked()
    }
}
