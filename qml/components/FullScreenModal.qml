import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

Page {
    id: root
    property alias content: body.data
    signal backRequested
    background: Rectangle {
        color: Theme.p.background
    }
    Accessible.role: Accessible.Pane
    Accessible.name: qsTr("Full screen modal")
    Accessible.description: qsTr("Full screen details")
    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Spacing.lg
        RowLayout {
            IconButton {
                iconSource: "qrc:/icons/back.svg"
                Accessible.name: qsTr("Go back")
                onClicked: root.backRequested()
            }
            Text {
                Layout.fillWidth: true
                text: root.title
                color: Theme.p.inkPrimary
                Component.onCompleted: Type.apply(this, Type.h1)
            }
        }
        ColumnLayout {
            id: body
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: Spacing.md
        }
    }
}
