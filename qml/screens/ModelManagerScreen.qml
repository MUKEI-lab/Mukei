import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"
import "../components"

Page {
    id: root

    property real storageValue: 0.43

    background: Rectangle {
        color: Theme.p.background
    }
    Accessible.role: Accessible.Pane
    Accessible.name: qsTr("Model manager")
    Accessible.description: qsTr("Switch, download, and remove local models")

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Spacing.lg
        spacing: Spacing.lg

        RowLayout {
            Layout.fillWidth: true
            IconButton {
                iconSource: "qrc:/icons/back.svg"
                Accessible.name: qsTr("Go back")
            }
            Text {
                Layout.fillWidth: true
                text: qsTr("Models")
                color: Theme.p.inkPrimary
                Component.onCompleted: Type.apply(this, Type.h1)
            }
        }

        Text {
            text: qsTr("INSTALLED")
            color: Theme.p.inkSecondary
            Component.onCompleted: Type.apply(this, Type.caption)
        }

        Rectangle {
            Layout.fillWidth: true
            radius: Theme.radiusLg
            color: Theme.p.surface
            implicitHeight: installedColumn.implicitHeight + Spacing.md * 2
            ColumnLayout {
                id: installedColumn
                anchors.fill: parent
                anchors.margins: Spacing.md
                spacing: Spacing.sm
                RowLayout {
                    Image {
                        source: "qrc:/icons/active-dot.svg"
                        Layout.preferredWidth: Spacing.md
                        Layout.preferredHeight: Spacing.md
                    }
                    Text {
                        Layout.fillWidth: true
                        text: qsTr("Gemma 3 4B Instruct")
                        color: Theme.p.inkPrimary
                        Component.onCompleted: Type.apply(this, Type.h3)
                    }
                }
                Text {
                    text: qsTr("2.5 GB · last used 12 min ago")
                    color: Theme.p.inkSecondary
                    Component.onCompleted: Type.apply(this, Type.caption)
                }
                RowLayout {
                    SecondaryButton {
                        text: qsTr("Switch")
                    }
                    GhostButton {
                        text: qsTr("Delete")
                    }
                }
            }
        }

        Text {
            text: qsTr("AVAILABLE")
            color: Theme.p.inkSecondary
            Component.onCompleted: Type.apply(this, Type.caption)
        }

        Rectangle {
            Layout.fillWidth: true
            radius: Theme.radiusLg
            color: Theme.p.surfaceFaint
            implicitHeight: availableColumn.implicitHeight + Spacing.md * 2
            ColumnLayout {
                id: availableColumn
                anchors.fill: parent
                anchors.margins: Spacing.md
                spacing: Spacing.sm
                Text {
                    text: qsTr("Llama 3.2 3B Instruct")
                    color: Theme.p.inkPrimary
                    Component.onCompleted: Type.apply(this, Type.h3)
                }
                Text {
                    text: qsTr("1.8 GB · Q4_K_M · Balanced for compact devices")
                    color: Theme.p.inkSecondary
                    wrapMode: Text.Wrap
                    Component.onCompleted: Type.apply(this, Type.bodySmall)
                }
                RowLayout {
                    SecondaryButton {
                        text: qsTr("Download")
                    }
                    GhostButton {
                        text: qsTr("Details")
                    }
                }
            }
        }

        Item {
            Layout.fillHeight: true
        }

        Text {
            text: qsTr("Storage: 5.2 / 12 GB used")
            color: Theme.p.inkSecondary
            Component.onCompleted: Type.apply(this, Type.caption)
        }
        ProgressBar {
            Layout.fillWidth: true
            value: root.storageValue
        }
    }
}
