import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import com.mukei.theme

Item {
    id: root
    property string kind: "system"
    property string label: ""
    property string phase: "active"
    property string iconSource: ""
    property string toolId: ""
    signal expanded(string toolId)
    Accessible.role: Accessible.StaticText
    Accessible.name: label
    Accessible.description: qsTr("Inline %1 event").arg(kind)
    implicitHeight: pill.implicitHeight + Spacing.xs
    Layout.fillWidth: true
    Layout.leftMargin: Spacing.sm
    Layout.rightMargin: Spacing.xl
    StatusPill { id: pill; anchors.verticalCenter: parent.verticalCenter; iconSource: root.iconSource; text: root.label; subtype: root.phase === "failure" ? "Failure" : root.phase === "result" ? "Success" : "ActiveTool" }
    TapHandler { enabled: root.kind === "tool" && root.phase === "result"; onTapped: root.expanded(root.toolId) }
}
