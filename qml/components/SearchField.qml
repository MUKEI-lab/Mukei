import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

Control {
    id: root
    property alias text: input.text
    signal accepted(string text)
    Accessible.role: Accessible.EditableText
    Accessible.name: qsTr("Search conversations")
    Accessible.description: qsTr("Type to filter conversations")
    implicitHeight: Spacing.xxl
    implicitWidth: Spacing.huge * 3
    contentItem: RowLayout {
        spacing: Spacing.xs
        Image {
            source: "qrc:/icons/search.svg"
            Layout.preferredWidth: Spacing.lg
            Layout.preferredHeight: Spacing.lg
        }
        TextField {
            id: input
            Layout.fillWidth: true
            background: null
            color: Theme.p.inkPrimary
            onAccepted: root.accepted(text)
        }
        IconButton {
            iconSource: "qrc:/icons/delete.svg"
            visible: input.text.length > 0
            Accessible.name: qsTr("Clear search")
            onClicked: input.clear()
        }
    }
    background: Rectangle {
        color: Theme.p.surface
        radius: Theme.radiusSm
        border.width: Theme.highContrast ? 1 : 0
        border.color: Theme.p.divider
    }
}
