import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"
import "../components"

Rectangle {
    id: root
    visible: LifecycleStore.interactive && !ResponsiveStore.compact
    width: ResponsiveStore.expanded ? 216 : 72
    color: Theme.p.surface
    border.width: 1
    border.color: Theme.p.divider

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Spacing.sm
        spacing: Spacing.xs

        Text {
            Layout.fillWidth: true
            visible: ResponsiveStore.expanded
            text: qsTr("Mukei")
            color: Theme.p.inkPrimary
            leftPadding: Spacing.sm
            Component.onCompleted: Type.apply(this, Type.h2)
        }

        Repeater {
            model: [
                { route: "chat", label: qsTr("Chat"), icon: "qrc:/icons/chat.svg" },
                { route: "models", label: qsTr("Models"), icon: "qrc:/icons/chip.svg" },
                { route: "downloads", label: qsTr("Downloads"), icon: "qrc:/icons/done-target.svg" },
                { route: "documents", label: qsTr("Documents"), icon: "qrc:/icons/file.svg" },
                { route: "settings", label: qsTr("Settings"), icon: "qrc:/icons/settings.svg" }
            ]
            delegate: Button {
                Layout.fillWidth: true
                implicitHeight: 52
                text: ResponsiveStore.expanded ? modelData.label : ""
                icon.source: modelData.icon
                icon.width: 22
                icon.height: 22
                display: ResponsiveStore.expanded ? AbstractButton.TextBesideIcon : AbstractButton.IconOnly
                Accessible.name: modelData.label
                checked: NavigationStore.currentRoute === modelData.route
                checkable: true
                autoExclusive: true
                onClicked: IntentDispatcher.dispatch({ type: "navigation.open", route: modelData.route })
                background: Rectangle {
                    radius: Theme.radiusLg
                    color: parent.checked || parent.hovered ? Theme.p.surfaceFaint : "transparent"
                    border.width: parent.visualFocus ? 1 : 0
                    border.color: Theme.p.accent
                }
                contentItem: RowLayout {
                    spacing: Spacing.sm
                    Image {
                        source: modelData.icon
                        sourceSize: Qt.size(22, 22)
                        Layout.alignment: Qt.AlignHCenter
                    }
                    Text {
                        visible: ResponsiveStore.expanded
                        Layout.fillWidth: true
                        text: modelData.label
                        color: Theme.p.inkPrimary
                        Component.onCompleted: Type.apply(this, Type.bodyUI)
                    }
                }
            }
        }
        Item { Layout.fillHeight: true }
        Text {
            Layout.fillWidth: true
            visible: ResponsiveStore.expanded && StorageStore.warning
            text: qsTr("Storage %1% full").arg(Math.round(StorageStore.usageRatio * 100))
            color: StorageStore.critical ? Theme.error : Theme.warning
            wrapMode: Text.Wrap
            leftPadding: Spacing.sm
            Component.onCompleted: Type.apply(this, Type.caption)
        }
    }
}
