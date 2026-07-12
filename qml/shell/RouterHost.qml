import QtQuick
import QtQuick.Controls.Basic
import "../theme"
import "../screens"

import "../architecture"
import "../stores"
Item {
    id: root

    Loader {
        id: routeLoader
        anchors.fill: parent
        asynchronous: false
        sourceComponent: {
            switch (NavigationStore.currentRoute) {
            case "welcome": return welcomePage
            case "recovery": return recoveryPage
            case "chat": return chatPage
            case "models": return modelsPage
            case "downloads": return downloadsPage
            case "documents": return documentsPage
            case "settings": return settingsPage
            case "diagnostics": return diagnosticsPage
            case "security": return securityPage
            case "compatibility": return compatibilityPage
            case "unlock": return unlockPage
            default: return bootPage
            }
        }

        opacity: status === Loader.Ready ? 1 : 0
        Behavior on opacity {
            enabled: !Theme.reduceMotion
            NumberAnimation { duration: Motion.contentChange; easing.type: Easing.OutCubic }
        }
    }

    Component {
        id: bootPage
        StartupScreen {
            titleText: LifecycleStore.title
            detailText: LifecycleStore.description
            busy: !LifecycleStore.quarantined
        }
    }

    Component {
        id: unlockPage
        StartupScreen {
            titleText: LifecycleStore.title
            detailText: LifecycleStore.description
            busy: true
        }
    }

    Component {
        id: welcomePage
        WelcomeScreen {
            onGetStarted: IntentDispatcher.dispatch({ type: "app.initialize" })
        }
    }

    Component { id: recoveryPage; RecoveryScreen {} }
    Component { id: chatPage; ChatScreen {} }
    Component { id: modelsPage; ModelManagerScreen {} }
    Component { id: downloadsPage; DownloadsScreen {} }
    Component { id: documentsPage; DocumentsScreen {} }
    Component { id: settingsPage; SettingsScreen {} }
    Component { id: diagnosticsPage; DiagnosticsScreen {} }
    Component { id: securityPage; SafeModeScreen {} }
    Component { id: compatibilityPage; CompatibilityScreen {} }
}
