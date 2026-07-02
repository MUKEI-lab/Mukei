import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"
import "../components"

Page {
    id: root
    property int phase: 0
    readonly property var labels: [qsTr("Verifying cryptographic integrity…"), qsTr("Extracting on-device assets…"), qsTr("Initializing private storage…")]
    background: Rectangle {
        color: Theme.p.background
    }
    Accessible.role: Accessible.Pane
    Accessible.name: qsTr("Verification screen")
    Accessible.description: labels[phase]
    ColumnLayout {
        anchors.centerIn: parent
        width: parent.width - Spacing.xl * 2
        spacing: Spacing.md
        Spinner {
            Layout.alignment: Qt.AlignHCenter
        }
        Text {
            Layout.fillWidth: true
            text: root.labels[root.phase]
            color: Theme.p.inkPrimary
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.bodyUI)
        }
        Text {
            Layout.fillWidth: true
            text: qsTr("This guarantees the model was not tampered with during download.")
            color: Theme.p.inkSecondary
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.bodySmall)
        }
    }
}
