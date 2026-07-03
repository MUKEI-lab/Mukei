import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"
import "../components"

Page {
    id: root
    property var catalogue: []
    background: Rectangle {
        color: Theme.p.background
    }
    Accessible.role: Accessible.Pane
    Accessible.name: qsTr("Model picker")
    Accessible.description: qsTr("Choose a model to run locally")
    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Spacing.lg
        spacing: Spacing.md
        RowLayout {
            IconButton {
                iconSource: "qrc:/icons/back.svg"
                Accessible.name: qsTr("Go back")
            }
            Text {
                Layout.fillWidth: true
                text: qsTr("Choose a Model")
                color: Theme.p.inkPrimary
                Component.onCompleted: Type.apply(this, Type.h1)
            }
        }
        Repeater {
            model: catalogue.length > 0 ? catalogue : [{
                    "name": "Gemma 3 4B Instruct",
                    "size": "2.5 GB",
                    "quant": "Q4_K_M",
                    "blurb": "Fast. Good for general chat."
                }]
            delegate: Rectangle {
                Layout.fillWidth: true
                radius: Theme.radiusLg
                color: Theme.p.surface
                implicitHeight: cardColumn.implicitHeight + Spacing.md * 2
                ColumnLayout {
                    id: cardColumn
                    anchors.fill: parent
                    anchors.margins: Spacing.md
                    Text {
                        text: modelData.name
                        color: Theme.p.inkPrimary
                        Component.onCompleted: Type.apply(this, Type.h3)
                    }
                    Text {
                        text: modelData.size + qsTr(" · ") + modelData.quant
                        color: Theme.p.inkSecondary
                        Component.onCompleted: Type.apply(this, Type.bodySmall)
                    }
                    Text {
                        text: modelData.blurb
                        color: Theme.p.inkSecondary
                        wrapMode: Text.Wrap
                        Component.onCompleted: Type.apply(this, Type.bodySmallItalic)
                    }
                    SecondaryButton {
                        text: qsTr("Download")
                    }
                }
            }
        }
        GhostButton {
            text: qsTr("Use a custom GGUF file")
        }
    }
}
