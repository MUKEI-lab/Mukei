import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../architecture"
import "../stores"
import "../theme"
import "../components"

Page {
    id: root
    background: Rectangle { color: Theme.p.background }
    Accessible.role: Accessible.Pane
    Accessible.name: qsTr("Compatibility check failed")

    ColumnLayout {
        anchors.centerIn: parent
        width: Math.min(parent.width - Spacing.xl * 2, 620)
        spacing: Spacing.lg

        MukeiIcon {
            Layout.alignment: Qt.AlignHCenter
            name: "error"
            size: 32
            Accessible.ignored: true
        }
        Text {
            Layout.fillWidth: true
            text: qsTr("Mukei components need to be updated together")
            color: Theme.p.inkPrimary
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.h1)
        }
        Text {
            Layout.fillWidth: true
            text: ContractStore.safeMessage
            color: Theme.p.inkSecondary
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.bodyUI)
        }
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: details.implicitHeight + Spacing.lg * 2
            radius: Theme.radiusLg
            color: Theme.p.surface
            border.width: 1
            border.color: Theme.p.divider
            Text {
                id: details
                anchors.fill: parent
                anchors.margins: Spacing.lg
                text: qsTr("Frontend contract: %1\nBridge contract: %2\nSupported frontend range: %3–%4")
                    .arg(ContractStore.qmlContractVersion)
                    .arg(ContractStore.bridgeContractVersion || qsTr("unknown"))
                    .arg(ContractStore.minimumQmlVersion || qsTr("unknown"))
                    .arg(ContractStore.maximumQmlVersion || qsTr("unknown"))
                color: Theme.p.inkSecondary
                wrapMode: Text.Wrap
                Component.onCompleted: Type.apply(this, Type.bodySmall)
            }
        }
        PrimaryButton {
            Layout.alignment: Qt.AlignHCenter
            text: qsTr("Check again")
            onClicked: IntentDispatcher.dispatch({ type: "contract.retry" })
        }
    }
}
