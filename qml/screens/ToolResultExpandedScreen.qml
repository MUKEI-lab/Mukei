import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import com.mukei.theme
import "../components"

Page {
    id: root
    background: Rectangle { color: Theme.p.background }
    Accessible.role: Accessible.Pane
    Accessible.name: qsTr("Tool result")
    Accessible.description: qsTr("Inspect raw tool output and copy diagnostics.")
    ColumnLayout { anchors.fill: parent; anchors.margins: Spacing.lg; spacing: Spacing.md; Text { Layout.fillWidth: true; text: qsTr("Tool result"); color: Theme.p.inkPrimary; wrapMode: Text.Wrap; Component.onCompleted: Type.apply(this, Type.h1) } Text { Layout.fillWidth: true; text: qsTr("Inspect raw tool output and copy diagnostics."); color: Theme.p.inkSecondary; wrapMode: Text.Wrap; Component.onCompleted: Type.apply(this, Type.bodyUI) } PrimaryButton { text: qsTr("Continue") } GhostButton { text: qsTr("Close") } }
}
