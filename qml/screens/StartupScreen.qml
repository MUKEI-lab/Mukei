import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"
import "../components"

Page {
    id: root
    property string titleText: qsTr("Starting Mukei")
    property string detailText: qsTr("Preparing the private local runtime.")
    property bool busy: true

    background: Rectangle { color: Theme.p.background }
    Accessible.role: Accessible.Pane
    Accessible.name: root.titleText
    Accessible.description: root.detailText

    ColumnLayout {
        anchors.centerIn: parent
        width: Math.min(parent.width - Spacing.xl * 2, Spacing.huge * 5)
        spacing: Spacing.lg

        Spinner {
            Layout.alignment: Qt.AlignHCenter
            visible: root.busy
        }
        Text {
            Layout.fillWidth: true
            text: root.titleText
            color: Theme.p.inkPrimary
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.h2)
        }
        Text {
            Layout.fillWidth: true
            text: root.detailText
            color: Theme.p.inkSecondary
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.bodyUI)
        }
        StatusPill {
            Layout.alignment: Qt.AlignHCenter
            text: qsTr("Local and encrypted")
            subtype: "Network-Offline"
            iconSource: "qrc:/icons/lock.svg"
        }
    }
}
