pragma Singleton
import QtQuick
import "../stores"

QtObject {
    readonly property var contract: ContractStore
    readonly property var lifecycle: LifecycleStore
    readonly property var navigation: NavigationStore
    readonly property var capabilities: CapabilityStore
    readonly property var conversations: ConversationStore
    readonly property var chat: ChatStore
    readonly property var models: ModelStore
    readonly property var downloads: DownloadStore
    readonly property var documents: DocumentStore
    readonly property var storage: StorageStore
    readonly property var settings: SettingsStore
    readonly property var responsive: ResponsiveStore
    readonly property var recovery: RecoveryStore
    readonly property var operations: OperationStore
    readonly property var errors: ErrorStore
    readonly property var diagnostics: DiagnosticsStore
    readonly property var accessibility: AccessibilityStore
    readonly property var uiSession: UiSessionStore
}
