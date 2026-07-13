pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

Column {
    id: root
    property var ast: []
    property string fallbackText: ""
    spacing: Spacing.sm

    Repeater {
        model: root.ast && root.ast.length > 0 ? root.ast : [{
                "type": "Paragraph",
                "text": root.fallbackText
            }]
        delegate: Column {
            id: markdownNode
            required property var modelData
            width: root.width

            Text {
                width: markdownNode.width
                visible: markdownNode.modelData.type !== "CodeBlock"
                         && markdownNode.modelData.type !== "HorizontalRule"
                text: markdownNode.modelData.text || ""
                wrapMode: Text.Wrap
                textFormat: Text.PlainText
                color: Theme.p.inkPrimary
                Component.onCompleted: Type.apply(this,
                                                  markdownNode.modelData.type === "Heading"
                                                  ? Type.h2 : Type.bodyAI)
            }

            CodeBlockComponent {
                width: markdownNode.width
                visible: markdownNode.modelData.type === "CodeBlock"
                code: markdownNode.modelData.text || ""
                language: markdownNode.modelData.lang || ""
            }

            Rectangle {
                width: markdownNode.width
                height: visible ? 1 : 0
                visible: markdownNode.modelData.type === "HorizontalRule"
                color: Theme.p.divider
            }
        }
    }
}
