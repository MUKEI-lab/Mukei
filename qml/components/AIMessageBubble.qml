import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtQuick.Accessibility
import com.mukei.theme
Item { id: root; property string text: ""; property string timestamp: ""; property string suggestedAction: ""; property bool readerWash: implicitHeight > 320; implicitHeight: col.implicitHeight; Column { id: col; width: parent.width; Rectangle { width: parent.width; implicitHeight: body.implicitHeight + Spacing.md*2; radius: Theme.radiusMd; color: root.readerWash ? Theme.p.surfaceFaint : "transparent"; Text { id: body; anchors.fill: parent; anchors.margins: Spacing.md; text: root.text; wrapMode: Text.Wrap; color: Theme.p.inkPrimary; font: Type.bodyAI } } Row { spacing: Spacing.sm; Text { text: root.timestamp; color: Theme.p.inkFaint; font: Type.caption } StatusPill { visible: root.suggestedAction.length > 0; text: root.suggestedAction; subtype: "Action" } } } }
