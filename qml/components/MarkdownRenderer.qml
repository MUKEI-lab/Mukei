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
        delegate: Loader {
            width: root.width
            sourceComponent: modelData.type === "CodeBlock" ? codeBlockComponent : modelData.type === "HorizontalRule" ? ruleComponent : textComponent
            property var node: modelData
        }
    }
    Component {
        id: textComponent
        Text {
            width: root.width
            text: node.text || ""
            wrapMode: Text.Wrap
            textFormat: Text.PlainText
            color: Theme.p.inkPrimary
            Component.onCompleted: Type.apply(this, node.type === "Heading" ? Type.h2 : Type.bodyAI)
        }
    }
    Component {
        id: codeBlockComponent
        CodeBlockComponent {
            width: root.width
            code: node.text || ""
            language: node.lang || ""
        }
    }
    Component {
        id: ruleComponent
        Rectangle {
            width: root.width
            height: 1
            color: Theme.p.divider
        }
    }
}
