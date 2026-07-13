pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

ModalSheet {
    id: root
    property var branches: []
    signal branchSelected(string id)
    content: ColumnLayout {
        spacing: Spacing.md
        Text {
            text: qsTr("Branches")
            color: Theme.p.inkPrimary
            Component.onCompleted: Type.apply(this, Type.h2)
        }
        Repeater {
            model: root.branches
            delegate: RowLayout {
                id: branchRow
                required property var modelData
                spacing: Spacing.sm
                Image {
                    source: "qrc:/icons/active-dot.svg"
                    visible: branchRow.modelData.active === true
                    Layout.preferredWidth: Spacing.md
                    Layout.preferredHeight: Spacing.md
                }
                GhostButton {
                    text: branchRow.modelData.title || qsTr("Branch")
                    onClicked: root.branchSelected(branchRow.modelData.id || "")
                }
            }
        }
    }
}
