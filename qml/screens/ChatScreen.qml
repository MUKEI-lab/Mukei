import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtQuick.Accessibility
import com.mukei.theme
import "../components"
Page { id: root; title: qsTr("Mukei"); background: Rectangle { color: Theme.p.background }
ColumnLayout { anchors.fill: parent; anchors.margins: Spacing.md; spacing: Spacing.sm
RowLayout { Layout.fillWidth: true; IconButton { iconSource: "qrc:/icons/chat.svg"; Accessible.name: qsTr("Open drawer") } Text { Layout.fillWidth: true; text: root.title; color: Theme.p.inkPrimary; font: Type.h3 } IconButton { iconSource: "qrc:/icons/settings.svg"; Accessible.name: qsTr("Settings") } }
Flickable { id: chatFlickable; objectName: "chatFlickable"; Layout.fillWidth: true; Layout.fillHeight: true; contentWidth: width; contentHeight: timeline.implicitHeight; clip: true; ColumnLayout { id: timeline; objectName: "timeline"; width: chatFlickable.width; spacing: Spacing.lg; UserMessageBubble { Layout.fillWidth: true; text: qsTr("what is entropy?"); timestamp: qsTr("Now") } ChatTimelineEvent { Layout.fillWidth: true; kind: "thinking"; label: qsTr("Thinking (collapsed)"); phase: "result" } AIMessageBubble { Layout.fillWidth: true; text: qsTr("Entropy is a measure of uncertainty or disorder in a system."); timestamp: qsTr("Now") } ChatTimelineEvent { Layout.fillWidth: true; kind: "tool"; label: qsTr("Searching web…"); phase: "active"; iconSource: "qrc:/icons/search.svg" } } }
NetworkBanner { Layout.fillWidth: true; text: qsTr("🔒 local-only · Network: off—you are private") } ChatComposer { Layout.fillWidth: true } } }
