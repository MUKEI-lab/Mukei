import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

Item {
    id: root
    property string text: ""
    property string timestamp: ""
    signal actionRequested(string action)
    Accessible.role: Accessible.StaticText
    Accessible.name: qsTr("User message")
    Accessible.description: text
    implicitHeight: column.implicitHeight
    Layout.alignment: Qt.AlignRight
    ColumnLayout {
        id: column
        anchors.right: parent.right
        width: Math.min(parent ? parent.width * 0.78 : Spacing.huge * 3, bubble.implicitWidth)
        spacing: Spacing.xxs
        Rectangle {
            id: bubble
            Layout.fillWidth: true
            implicitHeight: message.implicitHeight + Spacing.md * 2
            radius: Theme.radiusMd
            color: Theme.p.surfaceVariant
            Behavior on color {
                enabled: !Theme.reduceMotion
                ColorAnimation { 
                    duration: Motion.themeCrossFade
                    easing.type: Easing.BezierSpline
                    easing.bezierCurve: Motion.enter
                }
            }
            opacity: 0
            scale: 0.98
            Component.onCompleted: { opacity = 1; scale = 1.0 }
            Behavior on opacity {
                enabled: !Theme.reduceMotion
                NumberAnimation { 
                    duration: Motion.bubbleAppear
                    easing.type: Easing.BezierSpline
                    easing.bezierCurve: Motion.enter
                }
            }
            Behavior on scale {
                enabled: !Theme.reduceMotion
                NumberAnimation { 
                    duration: Motion.bubbleAppear
                    easing.type: Easing.BezierSpline
                    easing.bezierCurve: Motion.enter
                }
            }
            Text {
                id: message
                anchors.fill: parent
                anchors.margins: Spacing.md
                text: root.text
                wrapMode: Text.Wrap
                color: Theme.p.inkPrimary
                // Live bindings for font properties so they react to Theme.scale changes
                font.family: Type.bodyUI.family
                font.pixelSize: Type.bodyUI.pixelSize
                font.weight: Type.bodyUI.weight
                font.italic: Type.bodyUI.italic
                lineHeightMode: Text.ProportionalHeight
                lineHeight: Type.bodyUI.lineHeight
            }
        }
        Text {
            text: root.timestamp
            color: Theme.p.inkFaint
            // Live bindings for font properties so they react to Theme.scale changes
            font.family: Type.caption.family
            font.pixelSize: Type.caption.pixelSize
            font.weight: Type.caption.weight
            font.italic: Type.caption.italic
            lineHeightMode: Text.ProportionalHeight
            lineHeight: Type.caption.lineHeight
        }
    }
    TapHandler {
        acceptedButtons: Qt.RightButton
        onTapped: root.actionRequested("Edit")
    }
}
