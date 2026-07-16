import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../architecture"
import "../stores"
import "../theme"
import "../components"

Page {
    id: root

    background: Rectangle {
        color: Theme.p.background
    }
    Accessible.role: Accessible.Pane
    Accessible.name: qsTr("Safe mode")
    Accessible.description: qsTr("Recover calmly from repeated crashes")

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Spacing.xl
        spacing: Spacing.lg

        Item {
            Layout.preferredHeight: Spacing.xl
        }
        Text {
            Layout.fillWidth: true
            text: qsTr("We've had a few crashes.
What now?")
            color: Theme.p.inkPrimary
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.display)
        }
        Text {
            Layout.fillWidth: true
            text: qsTr("Mukei detected 2 unexpected closures in the last 24 hours. You can continue anyway, or reset all data to start fresh. Your model file will be kept either way.")
            color: Theme.p.inkSecondary
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.bodyUI)
        }
        PrimaryButton {
            Layout.fillWidth: true
            text: qsTr("Continue Anyway")
            onClicked: {
                ErrorStore.dismiss()
                LifecycleStore.setLocalState("degraded", qsTr("Mukei is open in limited mode because native startup did not finish."))
                NavigationStore.syncWithLifecycle(LifecycleStore.state)
            }
        }
        DestructiveButton {
            Layout.fillWidth: true
            text: qsTr("Reset All Data")
            onClicked: ErrorStore.push({
                code: "ERR_RESET_REQUIRES_REINSTALL",
                severity: "error",
                recoverable: true,
                user_message: qsTr("Automatic reset is not available in this build. Uninstall Mukei, then install the corrected APK.")
            }, "ERR_RESET_REQUIRES_REINSTALL")
        }
        GhostButton {
            text: qsTr("View Crash Log")
            onClicked: {
                ErrorStore.dismiss()
                NavigationStore.navigate("diagnostics", ({ from: "safe_mode" }), false)
            }
        }
        Item {
            Layout.fillHeight: true
        }
    }
}
