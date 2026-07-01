import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtQuick.Accessibility
import com.mukei.theme
Item { id: root; property string text: ""; property string timestamp: ""; implicitHeight: col.implicitHeight; Layout.alignment: Qt.AlignRight; Column { id: col; anchors.right: parent.right; width: Math.min(parent.width * 0.78, bubble.implicitWidth); Rectangle { id: bubble; radius: 12; color: Theme.p.surfaceVariant; implicitWidth: msg.implicitWidth + Spacing.lg*2; implicitHeight: msg.implicitHeight + Spacing.md*2; Text { id: msg; anchors.fill: parent; anchors.margins: Spacing.md; text: root.text; wrapMode: Text.Wrap; color: Theme.p.inkPrimary; font: Type.bodyUI } } Text { text: root.timestamp; color: Theme.p.inkFaint; font: Type.caption } } }
