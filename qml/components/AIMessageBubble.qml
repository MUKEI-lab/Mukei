import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

Item {
    id: root
    property string text: ""
    property var ast: []
    property string timestamp: ""
    property bool containsCodeBlock: false
    property string suggestedAction: ""
    signal actionRequested(string action)
    readonly property bool readerWash: body.implicitHeight > Spacing.huge * 3 + Spacing.xl || Theme.scaleClass === "large" || containsCodeBlock
    Accessible.role: Accessible.StaticText
    Accessible.name: qsTr("Mukei response")
    Accessible.description: text
    implicitHeight: column.implicitHeight
    ColumnLayout {
        id: column
        width: parent ? parent.width : Spacing.huge * 4
        spacing: Spacing.xs
        Rectangle {
            id: body
            Layout.fillWidth: true
            implicitHeight: renderer.implicitHeight + Spacing.md * 2
            radius: Theme.radiusLg
            color: root.readerWash ? Theme.p.surfaceFaint : "transparent"
            Behavior on color {
                enabled: !Theme.reduceMotion
                ColorAnimation { duration: Motion.bubbleAppear; easing.type: Easing.BezierSpline; easing.bezierCurve: Motion.enter }
            }
            opacity: 0
            Component.onCompleted: opacity = 1
            Behavior on opacity {
                enabled: !Theme.reduceMotion
                NumberAnimation { duration: Motion.bubbleAppear; easing.type: Easing.BezierSpline; easing.bezierCurve: Motion.enter }
            }
            MarkdownRenderer {
                id: renderer
                anchors.fill: parent
                anchors.margins: Spacing.md
                ast: root.ast
                fallbackText: root.text
            }
        }
        RowLayout {
            spacing: Spacing.sm
            Text {
                text: root.timestamp
                color: Theme.p.inkFaint
                Component.onCompleted: Type.apply(this, Type.caption)
            }
            StatusPill {
                visible: root.suggestedAction.length > 0
                text: root.suggestedAction
                subtype: "Action"
            }
        }
    }
    TapHandler {
        acceptedButtons: Qt.RightButton
        onTapped: root.actionRequested("Copy text")
    }
}
