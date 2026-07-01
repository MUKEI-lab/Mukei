import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtQuick.Accessibility
import com.mukei.theme
import "../components"
Page { id: root; title: qsTr("Verifying cryptographic integrity…"); background: Rectangle { color: Theme.p.background } ColumnLayout { anchors.fill: parent; anchors.margins: Spacing.xl; spacing: Spacing.lg; Text { text: root.title; color: Theme.p.inkPrimary; font: Type.h1; wrapMode: Text.Wrap } Text { text: qsTr("Local-first, private, editorial interface scaffold."); color: Theme.p.inkSecondary; font: Type.bodyUI; wrapMode: Text.Wrap } PrimaryButton { text: qsTr("Continue") } } }
