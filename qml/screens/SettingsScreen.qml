import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"
import "../components"

import "../architecture"
import "../stores"
Page {
    id: root
    property int selectedTab: 0
    readonly property var tabs: [qsTr("General"), qsTr("Privacy"), qsTr("Storage"), qsTr("About")]
    background: Rectangle { color: Theme.p.background }
    Accessible.role: Accessible.Pane
    Accessible.name: qsTr("Settings")

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: ResponsiveStore.compact ? Spacing.md : Spacing.xl
        spacing: Spacing.lg

        RowLayout {
            Layout.fillWidth: true
            IconButton {
                visible: ResponsiveStore.compact
                iconSource: "qrc:/icons/back.svg"
                text: qsTr("Back")
                onClicked: IntentDispatcher.dispatch({ type: "navigation.back" })
            }
            Text {
                Layout.fillWidth: true
                text: qsTr("Settings")
                color: Theme.p.inkPrimary
                Component.onCompleted: Type.apply(this, Type.h1)
            }
        }

        ScrollView {
            Layout.fillWidth: true
            implicitHeight: 52
            contentWidth: tabsRow.implicitWidth
            ScrollBar.vertical.policy: ScrollBar.AlwaysOff
            ScrollBar.horizontal.policy: ScrollBar.AsNeeded
            RowLayout {
                id: tabsRow
                spacing: Spacing.xs
                Repeater {
                    model: root.tabs
                    delegate: GhostButton {
                        text: modelData
                        active: root.selectedTab === index
                        onClicked: root.selectedTab = index
                    }
                }
            }
        }

        StackLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            currentIndex: root.selectedTab

            ScrollView {
                clip: true
                ColumnLayout {
                    width: Math.min(parent.width, 760)
                    spacing: Spacing.lg
                    Text { text: qsTr("Appearance"); color: Theme.p.inkPrimary; Component.onCompleted: Type.apply(this, Type.h2) }
                    RowLayout {
                        Layout.fillWidth: true
                        SecondaryButton { text: qsTr("Dolce Vita"); onClicked: IntentDispatcher.dispatch({ type: "settings.update", key: "theme_mode", value: "dolce_vita" }) }
                        SecondaryButton { text: qsTr("Espresso"); onClicked: IntentDispatcher.dispatch({ type: "settings.update", key: "theme_mode", value: "espresso" }) }
                        SecondaryButton { text: qsTr("Taupe"); onClicked: IntentDispatcher.dispatch({ type: "settings.update", key: "theme_mode", value: "taupe" }) }
                    }
                    CheckBox {
                        text: qsTr("Reduce motion")
                        checked: SettingsStore.reduceMotion
                        onToggled: IntentDispatcher.dispatch({ type: "settings.update", key: "reduce_motion", value: checked })
                    }
                    CheckBox {
                        text: qsTr("High contrast")
                        checked: SettingsStore.highContrast
                        onToggled: IntentDispatcher.dispatch({ type: "settings.update", key: "high_contrast", value: checked })
                    }
                    RowLayout {
                        Layout.fillWidth: true
                        Text { text: qsTr("Text size"); color: Theme.p.inkSecondary; Component.onCompleted: Type.apply(this, Type.bodyUI) }
                        Slider {
                            Layout.fillWidth: true
                            from: 85; to: 200; stepSize: 5
                            value: SettingsStore.fontScalePercent
                            onMoved: Theme.scale = value / 100
                            onPressedChanged: if (!pressed) IntentDispatcher.dispatch({ type: "settings.update", key: "font_scale_percent", value: Math.round(value) })
                            Accessible.name: qsTr("Text size percentage")
                        }
                        Text { text: Math.round(SettingsStore.fontScalePercent) + "%"; color: Theme.p.inkSecondary; Component.onCompleted: Type.apply(this, Type.caption) }
                    }

                    Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; color: Theme.p.divider }
                    Text { text: qsTr("Inference defaults"); color: Theme.p.inkPrimary; Component.onCompleted: Type.apply(this, Type.h2) }
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
                    Item { Layout.preferredHeight: Spacing.xl }
                }
            }

            ScrollView {
                clip: true
                ColumnLayout {
                    width: Math.min(parent.width, 760)
                    spacing: Spacing.lg
                    Text { text: qsTr("Privacy mode"); color: Theme.p.inkPrimary; Component.onCompleted: Type.apply(this, Type.h2) }
                    Text {
                        Layout.fillWidth: true
                        text: qsTr("Local-only is the safest default. Remote tools remain blocked unless you explicitly allow them.")
                        color: Theme.p.inkSecondary
                        wrapMode: Text.Wrap
                        Component.onCompleted: Type.apply(this, Type.bodyUI)
                    }
                    RadioButton {
                        text: qsTr("Local only")
                        checked: SettingsStore.remotePolicy === "local_only"
                        onClicked: IntentDispatcher.dispatch({ type: "settings.update", key: "remote_feature_policy", value: "local_only" })
                    }
                    RadioButton {
                        text: qsTr("Ask before remote access")
                        checked: SettingsStore.remotePolicy === "ask_before_remote"
                        onClicked: IntentDispatcher.dispatch({ type: "settings.update", key: "remote_feature_policy", value: "ask_before_remote" })
                    }
                    RadioButton {
                        text: qsTr("Allow remote tools")
                        checked: SettingsStore.remotePolicy === "remote_allowed"
                        onClicked: IntentDispatcher.dispatch({ type: "settings.update", key: "remote_feature_policy", value: "remote_allowed" })
                    }
                    StatusPill { text: qsTr("Encrypted local database"); subtype: "Success"; iconSource: "qrc:/icons/lock.svg" }
                    StatusPill { text: qsTr("No account required"); subtype: "Success"; iconSource: "qrc:/icons/check.svg" }
                    GhostButton { text: qsTr("Private documents"); onClicked: IntentDispatcher.dispatch({ type: "navigation.open", route: "documents" }) }
                    Item { Layout.preferredHeight: Spacing.xl }
                }
            }

            ScrollView {
                clip: true
                ColumnLayout {
                    width: Math.min(parent.width, 760)
                    spacing: Spacing.lg
                    StoragePressureCard { Layout.fillWidth: true }
                    Text {
                        text: qsTr("Verified models: %1").arg(StorageStore.formatBytes(StorageStore.modelBytes))
                        color: Theme.p.inkSecondary
                        Component.onCompleted: Type.apply(this, Type.bodyUI)
                    }
                    Text {
                        text: qsTr("Partial downloads: %1").arg(StorageStore.formatBytes(StorageStore.partialBytes))
                        color: Theme.p.inkSecondary
                        Component.onCompleted: Type.apply(this, Type.bodyUI)
                    }
                    RowLayout {
                        SecondaryButton { text: qsTr("Manage models"); onClicked: IntentDispatcher.dispatch({ type: "navigation.open", route: "models" }) }
                        GhostButton { text: qsTr("View downloads"); onClicked: IntentDispatcher.dispatch({ type: "navigation.open", route: "downloads" }) }
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
                    Text { text: qsTr("Mukei"); color: Theme.p.inkPrimary; Component.onCompleted: Type.apply(this, Type.h2) }
                    Text { text: qsTr("Version 0.7.5 · Local-first AI assistant"); color: Theme.p.inkSecondary; Component.onCompleted: Type.apply(this, Type.bodyUI) }
                    Text {
                        Layout.fillWidth: true
                        text: qsTr("Licensed under Apache License 2.0. Third-party components retain their own licenses.")
                        color: Theme.p.inkSecondary
                        wrapMode: Text.Wrap
                        Component.onCompleted: Type.apply(this, Type.bodyUI)
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
