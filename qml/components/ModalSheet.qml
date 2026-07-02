import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

Popup {
    id: root
    property alias content: contentHost.data
    modal: true
    focus: true
    width: parent ? parent.width : Spacing.huge * 4
    height: Math.min(contentHost.implicitHeight + Spacing.xl, parent ? parent.height * 0.8 : Spacing.huge * 6)
    y: parent ? parent.height - height : 0
    background: Rectangle {
        color: Theme.p.surface
        radius: Spacing.md
    }
    Overlay.modal: Rectangle {
        color: Theme.overlay
    }
    contentItem: Item {
        ColumnLayout {
            id: contentHost
            anchors.fill: parent
            anchors.margins: Spacing.lg
            spacing: Spacing.md
        }
    }
}
