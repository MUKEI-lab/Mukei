import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

ToolResultCard {
    id: root
    title: qsTr("Knowledge chunk")
    Rectangle {
        anchors.left: parent.left
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        width: 2
        color: Theme.p.accent
    }
}
