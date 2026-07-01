import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import com.mukei.theme
import "../components"

Page {
    id: root
    background: Rectangle { color: Theme.p.background }
    Accessible.role: Accessible.Pane
    Accessible.name: qsTr("We've had a few crashes. What now?")
    Accessible.description: qsTr("Continue anyway, reset data, or view the local crash log.")
    ColumnLayout { anchors.fill: parent; anchors.margins: Spacing.lg; spacing: Spacing.md; Text { Layout.fillWidth: true; text: qsTr("We've had a few crashes. What now?"); color: Theme.p.inkPrimary; wrapMode: Text.Wrap; Component.onCompleted: Type.apply(this, Type.h1) } Text { Layout.fillWidth: true; text: qsTr("Continue anyway, reset data, or view the local crash log."); color: Theme.p.inkSecondary; wrapMode: Text.Wrap; Component.onCompleted: Type.apply(this, Type.bodyUI) } PrimaryButton { text: qsTr("Continue") } GhostButton { text: qsTr("Close") } }
}
