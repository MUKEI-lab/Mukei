import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Qt5Compat.GraphicalEffects
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
        radius: Theme.radiusLg
        color: root.hovered || root.activeFocus ? Theme.p.surfaceFaint : "transparent"
        border.width: Theme.highContrast || root.activeFocus ? 1 : 0
        border.color: root.active ? Theme.p.accent : Theme.p.divider
    }

    contentItem: Item {
        Image {
            id: iconImage
            anchors.centerIn: parent
            source: root.iconSource
            sourceSize.width: Spacing.lg
            sourceSize.height: Spacing.lg
            fillMode: Image.PreserveAspectFit
            opacity: root.enabled ? 1 : 0.4
            visible: false
        }

        ColorOverlay {
            anchors.fill: iconImage
            source: iconImage
            color: root.active ? Theme.p.accent : Theme.p.inkPrimary
            opacity: root.enabled ? 1 : 0.4
        }
    }

    TapHandler {
        onTapped: root.clicked()
    }
}
