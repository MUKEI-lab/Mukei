import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../architecture"
import "../theme"
import "../components"

FullScreenModal {
    id: root
    objectName: "toolResultExpandedScreen"

    property string toolName: qsTr("Web Search Result")
    property string query: qsTr("today's space launches")
    property string rawJson: "[ { title: Space launch, source: local preview } ]"

    title: toolName
    Accessible.name: qsTr("Tool result details")
    Accessible.description: qsTr("Expanded raw tool output")
    onBackRequested: IntentDispatcher.dispatch({ type: "navigation.back" })

    content: ColumnLayout {
        spacing: Spacing.lg
        ToolResultCard {
            Layout.fillWidth: true
            title: qsTr("Query")
            body: root.query
        }
        ToolResultCard {
            Layout.fillWidth: true
            title: qsTr("Results · read-only")
            body: qsTr("Source: Brave + Tavily · trust: untrusted external data")
        }
        Text {
            text: qsTr("RAW")
            color: Theme.p.inkSecondary
            Component.onCompleted: Type.apply(this, Type.caption)
        }
        CodeBlockComponent {
            Layout.fillWidth: true
            code: root.rawJson
            language: "json"
        }
        RowLayout {
            Layout.alignment: Qt.AlignRight
            CopyButton {
                objectName: "toolResultCopyRawButton"
                textToCopy: root.rawJson
            }
            GhostButton {
                objectName: "toolResultCloseButton"
                text: qsTr("Close")
                onClicked: IntentDispatcher.dispatch({ type: "navigation.back" })
            }
        }
    }
}
