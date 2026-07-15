import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../architecture"
import "../stores"
import "../theme"
import "../components"

Rectangle {
    id: root

    visible: LifecycleStore.interactive && !ResponsiveStore.compact
    width: ResponsiveStore.expanded ? 264 : 80
    color: Theme.p.surface

    readonly property var destinations: [
        { route: "chat", label: qsTr("Chat"), icon: "chat" },
        { route: "models", label: qsTr("Models"), icon: "chip" },
        { route: "documents", label: qsTr("Documents"), icon: "file" },
        { route: "downloads", label: qsTr("Downloads"), icon: "done-target" },
        { route: "settings", label: qsTr("Settings"), icon: "settings" }
    ]

    ColumnLayout {
        anchors.fill: parent
        anchors.leftMargin: Spacing.sm
        anchors.rightMargin: Spacing.sm
        anchors.topMargin: Spacing.lg
        anchors.bottomMargin: Spacing.lg
        spacing: Spacing.xs

        Text {
            Layout.fillWidth: true
            Layout.leftMargin: Spacing.sm
            visible: ResponsiveStore.expanded
            text: qsTr("Mukei")
            color: Theme.p.inkPrimary
            Component.onCompleted: Type.apply(this, Type.h2)
        }

        Text {
            Layout.fillWidth: true
            Layout.leftMargin: Spacing.sm
            Layout.bottomMargin: Spacing.md
            visible: ResponsiveStore.expanded
            text: qsTr("Private by construction")
            color: Theme.p.inkSecondary
            Component.onCompleted: Type.apply(this, Type.caption)
        }

        Repeater {
            model: root.destinations
            delegate: Button {
                id: navigationButton
                required property var modelData

                Layout.fillWidth: true
                implicitHeight: 52
                checkable: true
                checked: NavigationStore.currentRoute === navigationButton.modelData.route
                Accessible.name: navigationButton.modelData.label
                onClicked: IntentDispatcher.dispatch({
                    type: "navigation.open",
                    route: navigationButton.modelData.route
                })

                background: Rectangle {
                    radius: Theme.radiusLg
                    color: navigationButton.checked
                           ? Theme.p.surfaceFaint
                           : navigationButton.down || navigationButton.hovered
                             ? Theme.p.surfaceVariant
                             : "transparent"
                    border.width: navigationButton.visualFocus ? 2 : 0
                    border.color: Theme.p.accent

                    Rectangle {
                        visible: navigationButton.checked
                        width: 3
                        height: parent.height - Spacing.md
                        radius: 2
                        anchors.left: parent.left
                        anchors.verticalCenter: parent.verticalCenter
                        color: Theme.p.accent
                    }
                }

                contentItem: RowLayout {
                    spacing: Spacing.sm

                    MukeiIcon {
                        name: navigationButton.modelData.icon
                        tone: navigationButton.checked ? Theme.p.accent : Theme.p.inkSecondary
                        Layout.preferredWidth: Spacing.lg
                        Layout.preferredHeight: Spacing.lg
                        Layout.alignment: Qt.AlignHCenter
                    }

                    Text {
                        visible: ResponsiveStore.expanded
                        Layout.fillWidth: true
                        text: navigationButton.modelData.label
                        color: navigationButton.checked ? Theme.p.inkPrimary : Theme.p.inkSecondary
                        Component.onCompleted: Type.apply(this, Type.bodyUI)
                    }
                }
            }
        }

        Item { Layout.fillHeight: true }

        StatusPill {
            Layout.alignment: Qt.AlignHCenter
            visible: ResponsiveStore.expanded
            text: qsTr("Local-only")
            subtype: "Network-Offline"
            iconSource: "qrc:/icons/lock.svg"
        }

        Text {
            Layout.fillWidth: true
            visible: ResponsiveStore.expanded && StorageStore.warning
            text: qsTr("Storage %1% full").arg(Math.round(StorageStore.usageRatio * 100))
            color: StorageStore.critical ? Theme.error : Theme.warning
            wrapMode: Text.Wrap
            horizontalAlignment: Text.AlignHCenter
            Component.onCompleted: Type.apply(this, Type.caption)
        }
    }
}
