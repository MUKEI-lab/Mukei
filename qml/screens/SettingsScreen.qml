pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../architecture"
import "../stores"
import "../theme"
import "../components"

Page {
    id: root

    property int selectedTab: 0
    readonly property var tabs: [qsTr("General"), qsTr("Privacy"), qsTr("Storage"), qsTr("About")]

    background: Rectangle { color: Theme.p.background }
    Accessible.role: Accessible.Pane
    Accessible.name: qsTr("Settings")

    ColumnLayout {
        anchors.fill: parent
        anchors.leftMargin: ResponsiveStore.edgePadding
        anchors.rightMargin: ResponsiveStore.edgePadding
        anchors.topMargin: Spacing.md
        anchors.bottomMargin: Spacing.md
        spacing: Spacing.lg

        RowLayout {
            Layout.fillWidth: true
            Layout.maximumWidth: ResponsiveStore.contentMaxWidth
            Layout.alignment: Qt.AlignHCenter

            IconButton {
                visible: ResponsiveStore.compact
                iconSource: "qrc:/icons/back.svg"
                text: qsTr("Back")
                onClicked: IntentDispatcher.dispatch({ type: "navigation.back" })
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 0
                Text {
                    text: qsTr("Settings")
                    color: Theme.p.inkPrimary
                    Component.onCompleted: Type.apply(this, Type.h1)
                }
                Text {
                    text: qsTr("Private preferences stored on this device")
                    color: Theme.p.inkSecondary
                    Component.onCompleted: Type.apply(this, Type.caption)
                }
            }
        }

        ScrollView {
            Layout.fillWidth: true
            Layout.maximumWidth: ResponsiveStore.contentMaxWidth
            Layout.alignment: Qt.AlignHCenter
            implicitHeight: 52
            contentWidth: tabsRow.implicitWidth
            ScrollBar.vertical.policy: ScrollBar.AlwaysOff
            ScrollBar.horizontal.policy: ScrollBar.AsNeeded

            RowLayout {
                id: tabsRow
                spacing: Spacing.xs

                Repeater {
                    model: root.tabs
                    delegate: Button {
                        id: tabButton
                        required property var modelData
                        required property int index
                        implicitHeight: 48
                        text: tabButton.modelData
                        checked: root.selectedTab === tabButton.index
                        checkable: true
                        onClicked: root.selectedTab = tabButton.index

                        background: Item {
                            Rectangle {
                                visible: tabButton.checked
                                height: 3
                                radius: 2
                                anchors.left: parent.left
                                anchors.right: parent.right
                                anchors.bottom: parent.bottom
                                color: Theme.p.accent
                            }
                        }

                        contentItem: Text {
                            text: tabButton.text
                            color: tabButton.checked ? Theme.p.inkPrimary : Theme.p.inkSecondary
                            horizontalAlignment: Text.AlignHCenter
                            verticalAlignment: Text.AlignVCenter
                            Component.onCompleted: Type.apply(this, Type.h3)
                        }
                    }
                }
            }
        }

        StackLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.maximumWidth: ResponsiveStore.contentMaxWidth
            Layout.alignment: Qt.AlignHCenter
            currentIndex: root.selectedTab

            ScrollView {
                clip: true
                ColumnLayout {
                    width: Math.min(parent.width, 760)
                    spacing: Spacing.lg

                    Text { text: qsTr("Appearance"); color: Theme.p.inkPrimary; Component.onCompleted: Type.apply(this, Type.h2) }

                    Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: appearanceColumn.implicitHeight + Spacing.lg * 2
                        radius: Theme.radiusXl
                        color: Theme.p.surface

                        ColumnLayout {
                            id: appearanceColumn
                            anchors.fill: parent
                            anchors.margins: Spacing.lg
                            spacing: Spacing.md

                            Text { text: qsTr("Theme"); color: Theme.p.inkSecondary; Component.onCompleted: Type.apply(this, Type.caption) }

                            GridLayout {
                                Layout.fillWidth: true
                                columns: ResponsiveStore.compact ? 1 : 3
                                columnSpacing: Spacing.xs
                                rowSpacing: Spacing.xs

                                SecondaryButton { Layout.fillWidth: true; text: qsTr("Dolce Vita"); onClicked: IntentDispatcher.dispatch({ type: "settings.update", key: "theme_mode", value: "dolce_vita" }) }
                                SecondaryButton { Layout.fillWidth: true; text: qsTr("Espresso"); onClicked: IntentDispatcher.dispatch({ type: "settings.update", key: "theme_mode", value: "espresso" }) }
                                SecondaryButton { Layout.fillWidth: true; text: qsTr("Taupe"); onClicked: IntentDispatcher.dispatch({ type: "settings.update", key: "theme_mode", value: "taupe" }) }
                            }

                            CheckBox { text: qsTr("Reduce motion"); checked: SettingsStore.reduceMotion; onToggled: IntentDispatcher.dispatch({ type: "settings.update", key: "reduce_motion", value: checked }) }
                            CheckBox { text: qsTr("High contrast"); checked: SettingsStore.highContrast; onToggled: IntentDispatcher.dispatch({ type: "settings.update", key: "high_contrast", value: checked }) }

                            Text { text: qsTr("Text size · %1%").arg(Math.round(SettingsStore.fontScalePercent)); color: Theme.p.inkSecondary; Component.onCompleted: Type.apply(this, Type.bodySmall) }
                            Slider {
                                Layout.fillWidth: true
                                from: 85
                                to: 200
                                stepSize: 5
                                value: SettingsStore.fontScalePercent
                                onMoved: Theme.scale = value / 100
                                onPressedChanged: if (!pressed) IntentDispatcher.dispatch({ type: "settings.update", key: "font_scale_percent", value: Math.round(value) })
                                Accessible.name: qsTr("Text size percentage")
                            }
                        }
                    }

                    Text { text: qsTr("Inference defaults"); color: Theme.p.inkPrimary; Component.onCompleted: Type.apply(this, Type.h2) }

                    Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: inferenceColumn.implicitHeight + Spacing.lg * 2
                        radius: Theme.radiusXl
                        color: Theme.p.surface

                        ColumnLayout {
                            id: inferenceColumn
                            anchors.fill: parent
                            anchors.margins: Spacing.lg
                            spacing: Spacing.md

                            Text {
                                Layout.fillWidth: true
                                text: qsTr("Changes apply to the next message and never alter an active response.")
                                color: Theme.p.inkSecondary
                                wrapMode: Text.Wrap
                                Component.onCompleted: Type.apply(this, Type.bodySmall)
                            }

                            SettingsTextField {
                                Layout.fillWidth: true
                                label: qsTr("Temperature")
                                text: (SettingsStore.temperatureMilli / 1000).toFixed(2)
                                inputMethodHints: Qt.ImhFormattedNumbersOnly
                                onEditingFinished: function(value) {
                                    var parsed = Math.max(0, Math.min(2000, Math.round(Number(value) * 1000)))
                                    IntentDispatcher.dispatch({ type: "settings.update", key: "temperature_milli", value: parsed })
                                }
                            }
                            SettingsTextField {
                                Layout.fillWidth: true
                                label: qsTr("Max tokens")
                                text: String(SettingsStore.maxTokens)
                                inputMethodHints: Qt.ImhDigitsOnly
                                onEditingFinished: function(value) {
                                    IntentDispatcher.dispatch({ type: "settings.update", key: "max_tokens_default", value: Math.max(64, Math.min(32768, Number(value))) })
                                }
                            }
                            SettingsTextField {
                                Layout.fillWidth: true
                                label: qsTr("Top-p")
                                text: (SettingsStore.topPMilli / 1000).toFixed(2)
                                inputMethodHints: Qt.ImhFormattedNumbersOnly
                                onEditingFinished: function(value) {
                                    var parsed = Math.max(1, Math.min(1000, Math.round(Number(value) * 1000)))
                                    IntentDispatcher.dispatch({ type: "settings.update", key: "top_p_milli", value: parsed })
                                }
                            }
                        }
                    }
                    Item { Layout.preferredHeight: Spacing.xl }
                }
            }

            ScrollView {
                clip: true
                ColumnLayout {
                    width: Math.min(parent.width, 760)
                    spacing: Spacing.lg

                    Text { text: qsTr("Privacy"); color: Theme.p.inkPrimary; Component.onCompleted: Type.apply(this, Type.h2) }

                    Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: privacyColumn.implicitHeight + Spacing.lg * 2
                        radius: Theme.radiusXl
                        color: Theme.p.surface

                        ColumnLayout {
                            id: privacyColumn
                            anchors.fill: parent
                            anchors.margins: Spacing.lg
                            spacing: Spacing.md

                            Text { Layout.fillWidth: true; text: qsTr("All data lives on this device."); color: Theme.p.inkPrimary; wrapMode: Text.Wrap; Component.onCompleted: Type.apply(this, Type.h3) }
                            Text { Layout.fillWidth: true; text: qsTr("No accounts. No telemetry. No background sync. Remote tools stay blocked unless you explicitly allow them."); color: Theme.p.inkSecondary; wrapMode: Text.Wrap; Component.onCompleted: Type.apply(this, Type.bodyUI) }
                            StatusPill { text: qsTr("Encrypted local database"); subtype: "Success"; iconSource: "qrc:/icons/lock.svg" }
                            StatusPill { text: qsTr("Crash logs stay on device"); subtype: "Success"; iconSource: "qrc:/icons/check.svg" }
                            StatusPill { text: qsTr("No account required"); subtype: "Success"; iconSource: "qrc:/icons/check.svg" }
                        }
                    }

                    Text { text: qsTr("Remote tools"); color: Theme.p.inkPrimary; Component.onCompleted: Type.apply(this, Type.h2) }
                    RadioButton { text: qsTr("Local only"); checked: SettingsStore.remotePolicy === "local_only"; onClicked: IntentDispatcher.dispatch({ type: "settings.update", key: "remote_feature_policy", value: "local_only" }) }
                    RadioButton { text: qsTr("Ask before remote access"); checked: SettingsStore.remotePolicy === "ask_before_remote"; onClicked: IntentDispatcher.dispatch({ type: "settings.update", key: "remote_feature_policy", value: "ask_before_remote" }) }
                    RadioButton { text: qsTr("Allow remote tools"); checked: SettingsStore.remotePolicy === "remote_allowed"; onClicked: IntentDispatcher.dispatch({ type: "settings.update", key: "remote_feature_policy", value: "remote_allowed" }) }
                    GhostButton { text: qsTr("Manage private documents"); onClicked: IntentDispatcher.dispatch({ type: "navigation.open", route: "documents" }) }
                    Item { Layout.preferredHeight: Spacing.xl }
                }
            }

            ScrollView {
                clip: true
                ColumnLayout {
                    width: Math.min(parent.width, 760)
                    spacing: Spacing.lg
                    Text { text: qsTr("Storage"); color: Theme.p.inkPrimary; Component.onCompleted: Type.apply(this, Type.h2) }
                    StoragePressureCard { Layout.fillWidth: true }

                    Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: storageColumn.implicitHeight + Spacing.lg * 2
                        radius: Theme.radiusXl
                        color: Theme.p.surface

                        ColumnLayout {
                            id: storageColumn
                            anchors.fill: parent
                            anchors.margins: Spacing.lg
                            spacing: Spacing.md
                            Text { text: qsTr("Verified models · %1").arg(StorageStore.formatBytes(StorageStore.modelBytes)); color: Theme.p.inkSecondary; Component.onCompleted: Type.apply(this, Type.bodyUI) }
                            Text { text: qsTr("Partial downloads · %1").arg(StorageStore.formatBytes(StorageStore.partialBytes)); color: Theme.p.inkSecondary; Component.onCompleted: Type.apply(this, Type.bodyUI) }
                            RowLayout {
                                Layout.fillWidth: true
                                SecondaryButton { Layout.fillWidth: true; text: qsTr("Manage models"); onClicked: IntentDispatcher.dispatch({ type: "navigation.open", route: "models" }) }
                                GhostButton { Layout.fillWidth: true; text: qsTr("Downloads"); onClicked: IntentDispatcher.dispatch({ type: "navigation.open", route: "downloads" }) }
                            }
                        }
                    }
                    GhostButton { text: qsTr("Refresh storage"); onClicked: IntentDispatcher.dispatch({ type: "storage.refresh" }) }
                    Item { Layout.preferredHeight: Spacing.xl }
                }
            }

            ScrollView {
                clip: true
                ColumnLayout {
                    width: Math.min(parent.width, 760)
                    spacing: Spacing.lg
                    Text { text: qsTr("Mukei"); color: Theme.p.inkPrimary; Component.onCompleted: Type.apply(this, Type.display) }
                    Text { text: qsTr("Version 0.7.5 · Local-first AI assistant"); color: Theme.p.inkSecondary; Component.onCompleted: Type.apply(this, Type.bodyUI) }

                    Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: aboutColumn.implicitHeight + Spacing.lg * 2
                        radius: Theme.radiusXl
                        color: Theme.p.surface
                        ColumnLayout {
                            id: aboutColumn
                            anchors.fill: parent
                            anchors.margins: Spacing.lg
                            spacing: Spacing.md
                            Text { Layout.fillWidth: true; text: qsTr("Calm. Capable. Confidential. Crafted."); color: Theme.p.inkPrimary; wrapMode: Text.Wrap; Component.onCompleted: Type.apply(this, Type.h3) }
                            Text { Layout.fillWidth: true; text: qsTr("Licensed under Apache License 2.0. Third-party components retain their own licenses."); color: Theme.p.inkSecondary; wrapMode: Text.Wrap; Component.onCompleted: Type.apply(this, Type.bodyUI) }
                            StatusPill { text: qsTr("On-device runtime"); subtype: "Success"; iconSource: "qrc:/icons/lock.svg" }
                        }
                    }
                    GhostButton { text: qsTr("Diagnostics"); onClicked: IntentDispatcher.dispatch({ type: "navigation.open", route: "diagnostics" }) }
                    Item { Layout.preferredHeight: Spacing.xl }
                }
            }
        }
    }

    Component.onCompleted: {
        SettingsStore.hydrate()
        StorageStore.hydrate()
    }
}
