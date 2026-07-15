import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"
import "../components"

Item {
    id: root

    signal promptFilled(string prompt)

    Accessible.role: Accessible.Pane
    Accessible.name: qsTr("Empty chat")
    Accessible.description: qsTr("Start a private on-device conversation")

    ColumnLayout {
        anchors.centerIn: parent
        width: Math.min(parent.width, 640)
        spacing: Spacing.md

        Text {
            Layout.fillWidth: true
            text: qsTr("Mukei is ready.")
            color: Theme.p.inkPrimary
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.display)
        }

        Text {
            Layout.fillWidth: true
            text: qsTr("Everything runs on your device.")
            color: Theme.p.inkSecondary
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.bodyUI)
        }

        StatusPill {
            Layout.alignment: Qt.AlignHCenter
            text: qsTr("Encrypted locally")
            subtype: "Network-Offline"
            iconSource: "qrc:/icons/lock.svg"
        }

        Item { Layout.preferredHeight: Spacing.lg }

        Text {
            Layout.fillWidth: true
            text: qsTr("Try one of these to start")
            color: Theme.p.inkSecondary
            horizontalAlignment: Text.AlignHCenter
            Component.onCompleted: Type.apply(this, Type.caption)
        }

        PromptCard {
            Layout.fillWidth: true
            prompt: qsTr("Summarize the concept of entropy.")
            onFillRequested: root.promptFilled(prompt)
        }
        PromptCard {
            Layout.fillWidth: true
            prompt: qsTr("Draft a privacy-first project plan.")
            onFillRequested: root.promptFilled(prompt)
        }
        PromptCard {
            Layout.fillWidth: true
            prompt: qsTr("Explain this note in plain language.")
            onFillRequested: root.promptFilled(prompt)
        }
    }
}
