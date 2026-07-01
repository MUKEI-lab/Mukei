import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

ModalSheet {
    id: root
    property string heading: qsTr("Are you sure?")
    property string body: ""
    signal confirmed
    content: ColumnLayout {
        spacing: Spacing.md
        Text {
            text: root.heading
            color: Theme.p.inkPrimary
            Component.onCompleted: Type.apply(this, Type.h2)
        }
        Text {
            text: root.body
            color: Theme.p.inkSecondary
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.bodyUI)
        }
        RowLayout {
            GhostButton {
                text: qsTr("Cancel")
                onClicked: root.close()
            }
            PrimaryButton {
                text: qsTr("Confirm")
                onClicked: root.confirmed()
            }
        }
    }
}
