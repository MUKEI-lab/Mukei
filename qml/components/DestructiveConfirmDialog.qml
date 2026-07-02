import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

ConfirmationDialog {
    id: root
    property string destructiveText: qsTr("Reset")
    signal destructiveCommitted
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
            DestructiveButton {
                text: root.destructiveText
                onCommitted: root.destructiveCommitted()
            }
        }
    }
}
