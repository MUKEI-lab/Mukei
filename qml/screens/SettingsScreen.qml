import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"
import "../components"

Page {
    id: root

    property int selectedTab: 0
    readonly property var tabs: [qsTr("General"), qsTr("Privacy"), qsTr("Storage"), qsTr("About")]

    background: Rectangle {
        color: Theme.p.background
    }
    Accessible.role: Accessible.Pane
    Accessible.name: qsTr("Settings")
    Accessible.description: qsTr("Theme, privacy, storage, and app information")

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Spacing.lg
        spacing: Spacing.lg

        Text {
            text: qsTr("Settings")
            color: Theme.p.inkPrimary
            Component.onCompleted: Type.apply(this, Type.h1)
        }

        RowLayout {
            Layout.fillWidth: true
            Repeater {
                model: root.tabs
                delegate: GhostButton {
                    text: modelData
                    activeFocusOnTab: true
                    onClicked: root.selectedTab = index
                    Rectangle {
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.bottom: parent.bottom
                        height: 2
                        color: index === root.selectedTab ? Theme.p.accent : "transparent"
                    }
                }
            }
        }

        StackLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            currentIndex: root.selectedTab

            ColumnLayout {
                spacing: Spacing.md
                Text {
                    text: qsTr("Theme")
                    color: Theme.p.inkSecondary
                    Component.onCompleted: Type.apply(this, Type.caption)
                }
                RowLayout {
                    SecondaryButton {
                        text: qsTr("Dolce Vita")
                    }
                    SecondaryButton {
                        text: qsTr("Espresso")
                    }
                    SecondaryButton {
                        text: qsTr("Taupe")
                    }
                }
                Text {
                    text: qsTr("Inference changes apply on the next message only.")
                    color: Theme.p.inkSecondary
                    wrapMode: Text.Wrap
                    Component.onCompleted: Type.apply(this, Type.bodySmall)
                }
                SettingsTextField {
                    Layout.fillWidth: true
                    label: qsTr("Temperature")
                    text: "0.7"
                }
                SettingsTextField {
                    Layout.fillWidth: true
                    label: qsTr("Max tokens")
                    text: "1024"
                }
                SettingsTextField {
                    Layout.fillWidth: true
                    label: qsTr("Top-p")
                    text: "0.95"
                }
            }

            ColumnLayout {
                spacing: Spacing.md
                Text {
                    text: qsTr("All data lives on this device.")
                    color: Theme.p.inkPrimary
                    wrapMode: Text.Wrap
                    Component.onCompleted: Type.apply(this, Type.h2)
                }
                StatusPill {
                    text: qsTr("No telemetry")
                    subtype: "Success"
                    iconSource: "qrc:/icons/lock.svg"
                }
                StatusPill {
                    text: qsTr("No accounts")
                    subtype: "Success"
                    iconSource: "qrc:/icons/check.svg"
                }
                StatusPill {
                    text: qsTr("No cloud sync")
                    subtype: "Success"
                    iconSource: "qrc:/icons/network-off.svg"
                }
                RowLayout {
                    GhostButton {
                        text: qsTr("View crash log")
                    }
                    DestructiveButton {
                        text: qsTr("Reset all data")
                    }
                }
            }

            ColumnLayout {
                spacing: Spacing.md
                Text {
                    text: qsTr("Local storage")
                    color: Theme.p.inkPrimary
                    Component.onCompleted: Type.apply(this, Type.h2)
                }
                Text {
                    text: qsTr("Models, vector index, cache, and encrypted conversations are stored locally.")
                    color: Theme.p.inkSecondary
                    wrapMode: Text.Wrap
                    Component.onCompleted: Type.apply(this, Type.bodyUI)
                }
                ProgressBar {
                    Layout.fillWidth: true
                    value: 0.43
                }
                RowLayout {
                    GhostButton {
                        text: qsTr("Clear cache")
                    }
                    GhostButton {
                        text: qsTr("Export conversation")
                    }
                }
            }

            ColumnLayout {
                spacing: Spacing.md
                Text {
                    text: qsTr("Mukei")
                    color: Theme.p.inkPrimary
                    Component.onCompleted: Type.apply(this, Type.h2)
                }
                Text {
                    text: qsTr("Version 0.7.5 · Local-first AI assistant")
                    color: Theme.p.inkSecondary
                    Component.onCompleted: Type.apply(this, Type.bodyUI)
                }
                GhostButton {
                    text: qsTr("Licenses")
                }
                GhostButton {
                    text: qsTr("Diagnostic export")
                }
            }
        }
    }
}
