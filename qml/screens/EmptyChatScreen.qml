import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"
import "../components"

Page {
    id: root
    signal promptFilled(string prompt)
    background: Rectangle {
        color: Theme.p.background
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Spacing.md
        spacing: Spacing.md
        RowLayout {
            Layout.fillWidth: true
            IconButton {
                iconSource: "qrc:/icons/chat.svg"
                Accessible.name: qsTr("Open drawer")
            }
            Text {
                Layout.fillWidth: true
                text: qsTr("Mukei")
                color: Theme.p.inkPrimary
                Component.onCompleted: Type.apply(this, Type.h3)
            }
            IconButton {
                iconSource: "qrc:/icons/settings.svg"
                Accessible.name: qsTr("Open settings")
            }
        }
        Item {
            Layout.preferredHeight: Spacing.xl
        }
        Text {
            text: qsTr("Mukei is ready.")
            color: Theme.p.inkPrimary
            Component.onCompleted: Type.apply(this, Type.display)
        }
        Text {
            text: qsTr("Everything runs on your device.")
            color: Theme.p.inkSecondary
            Component.onCompleted: Type.apply(this, Type.bodyUI)
        }
        StatusPill {
            text: qsTr("Encrypted locally")
            subtype: "Network-Offline"
            iconSource: "qrc:/icons/lock.svg"
        }
        Text {
            text: qsTr("Try one of these to start:")
            color: Theme.p.inkSecondary
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
        Item {
            Layout.fillHeight: true
        }
        NetworkBanner {
            Layout.fillWidth: true
        }
        ChatComposer {
            Layout.fillWidth: true
        }
    }
}
