import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../architecture"
import "../stores"
import "../theme"
import "../components"

Page {
    id: root
    background: Rectangle { color: Theme.p.background }
    Accessible.role: Accessible.Pane
    Accessible.name: qsTr("Diagnostics")

    readonly property var diagnosticRows: [
        { label: qsTr("Runtime"), value: DiagnosticsStore.snapshot.runtime_phase || LifecycleStore.state },
        { label: qsTr("Last startup stage"), value: LifecycleStore.previousState || LifecycleStore.state },
        { label: qsTr("Ready"), value: DiagnosticsStore.snapshot.ready === true ? qsTr("Yes") : qsTr("No") },
        { label: qsTr("Active operations"), value: String(OperationStore.activeCount) },
        { label: qsTr("Conversations"), value: String(ConversationStore.count) },
        { label: qsTr("Models in catalogue"), value: String(ModelStore.count) },
        { label: qsTr("Durable downloads"), value: String(DownloadStore.count) },
        { label: qsTr("Private document grants"), value: String(DiagnosticsStore.snapshot.document_grant_count || DocumentStore.count) },
        { label: qsTr("Cleanup pending"), value: String(DocumentStore.cleanupPendingCount) },
        { label: qsTr("Storage pressure"), value: StorageStore.pressure }
    ]

    ScrollView {
        anchors.fill: parent
        contentWidth: availableWidth
        ColumnLayout {
            width: Math.min(parent.width, 760)
            anchors.horizontalCenter: parent.horizontalCenter
            spacing: Spacing.lg

            Item { Layout.preferredHeight: Spacing.xl }

            RowLayout {
                Layout.fillWidth: true
                IconButton {
                    visible: ResponsiveStore.compact
                    iconSource: "qrc:/icons/back.svg"
                    text: qsTr("Back")
                    onClicked: IntentDispatcher.dispatch({ type: "navigation.back" })
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: Spacing.xxs
                    Text {
                        Layout.fillWidth: true
                        text: qsTr("Diagnostics")
                        color: Theme.p.inkPrimary
                        Component.onCompleted: Type.apply(this, Type.h1)
                    }
                    Text {
                        Layout.fillWidth: true
                        text: qsTr("Privacy-safe runtime status only")
                        color: Theme.p.inkSecondary
                        Component.onCompleted: Type.apply(this, Type.bodySmall)
                    }
                }
            }

            Text {
                Layout.fillWidth: true
                text: qsTr("This view and its export exclude prompts, document content, keys, tokens, provider responses, and private paths.")
                color: Theme.p.inkSecondary
                wrapMode: Text.Wrap
                Component.onCompleted: Type.apply(this, Type.bodyUI)
            }

            Repeater {
                model: root.diagnosticRows
                delegate: Rectangle {
                    id: diagnosticDelegate
                    required property var modelData
                    Layout.fillWidth: true
                    implicitHeight: Math.max(56, diagnosticRow.implicitHeight + Spacing.md * 2)
                    radius: Theme.radiusMd
                    color: Theme.p.surface
                    border.width: 1
                    border.color: Theme.p.divider
                    RowLayout {
                        id: diagnosticRow
                        anchors.fill: parent
                        anchors.margins: Spacing.md
                        Text {
                            Layout.fillWidth: true
                            text: diagnosticDelegate.modelData.label
                            color: Theme.p.inkSecondary
                            wrapMode: Text.Wrap
                            Component.onCompleted: Type.apply(this, Type.bodyUI)
                        }
                        Text {
                            text: diagnosticDelegate.modelData.value
                            color: Theme.p.inkPrimary
                            horizontalAlignment: Text.AlignRight
                            Component.onCompleted: Type.apply(this, Type.bodyUI)
                        }
                    }
                }
            }

            Rectangle {
                Layout.fillWidth: true
                implicitHeight: privacyText.implicitHeight + Spacing.lg * 2
                radius: Theme.radiusLg
                color: Theme.p.surfaceFaint
                border.width: 1
                border.color: Theme.p.divider
                Text {
                    id: privacyText
                    anchors.fill: parent
                    anchors.margins: Spacing.lg
                    text: qsTr("Export policy: prompts — no · documents — no · secrets — no · private paths — no")
                    color: Theme.p.inkSecondary
                    wrapMode: Text.Wrap
                    Component.onCompleted: Type.apply(this, Type.bodySmall)
                }
            }

            RowLayout {
                Layout.fillWidth: true
                SecondaryButton {
                    text: qsTr("Refresh")
                    onClicked: {
                        IntentDispatcher.dispatch({ type: "models.refresh" })
                        IntentDispatcher.dispatch({ type: "downloads.refresh" })
                        IntentDispatcher.dispatch({ type: "documents.refresh" })
                        IntentDispatcher.dispatch({ type: "storage.refresh" })
                        IntentDispatcher.dispatch({ type: "diagnostics.refresh" })
                    }
                }
                PrimaryButton {
                    text: DiagnosticsStore.exporting ? qsTr("Exporting…") : qsTr("Create safe report")
                    enabled: !DiagnosticsStore.exporting
                    onClicked: IntentDispatcher.dispatch({ type: "diagnostics.export" })
                }
                Item { Layout.fillWidth: true }
                GhostButton {
                    text: qsTr("Back")
                    onClicked: IntentDispatcher.dispatch({ type: "navigation.back" })
                }
            }

            Text {
                Layout.fillWidth: true
                visible: DiagnosticsStore.lastExportFilename.length > 0
                text: qsTr("Last report: %1").arg(DiagnosticsStore.lastExportFilename)
                color: Theme.p.inkFaint
                wrapMode: Text.Wrap
                Component.onCompleted: Type.apply(this, Type.caption)
            }

            Item { Layout.preferredHeight: Spacing.xl }
        }
    }

    Component.onCompleted: IntentDispatcher.dispatch({ type: "diagnostics.refresh" })
}
