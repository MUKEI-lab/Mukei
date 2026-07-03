import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

Control {
    id: root
    property string text: ""
    property bool expanded: false
    Accessible.role: Accessible.Button
    Accessible.name: expanded ? qsTr("Collapse thinking") : qsTr("Expand thinking")
    Accessible.description: qsTr("Mukei reasoning summary")
    implicitHeight: column.implicitHeight + Spacing.md
    background: Rectangle {
        color: Theme.p.surfaceFaint
        radius: Theme.radiusMd
        border.width: Theme.highContrast ? 1 : 0
        border.color: Theme.p.divider
    }
    contentItem: ColumnLayout {
        id: column
        spacing: Spacing.xs
        RowLayout {
            Image {
                source: root.expanded ? "qrc:/icons/collapse.svg" : "qrc:/icons/expand.svg"
                Layout.preferredWidth: Spacing.md
                Layout.preferredHeight: Spacing.md
            }
            Text {
                text: qsTr("Thinking")
                color: Theme.p.inkSecondary
                Component.onCompleted: Type.apply(this, Type.caption)
            }
        }
        Text {
            visible: root.expanded
            text: root.text
            color: Theme.p.inkSecondary
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.caption)
        }
    }
    TapHandler {
        onTapped: root.expanded = !root.expanded
    }
}
