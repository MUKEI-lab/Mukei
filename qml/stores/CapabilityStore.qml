pragma Singleton
import QtQuick

QtObject {
    property bool canInitialize: false
    property bool canSendMessage: false
    property bool canStopGeneration: false
    property bool canDownloadModel: false
    property bool canStopDownload: false
    property bool canSwitchModel: false
    property bool canDeleteModel: false
    property bool canClearConversation: false
    property bool canOpenSettings: true
    property bool needsConfig: false
    property bool needsStoragePermission: false
    property bool activeModelReady: false
    property bool isBusy: false
    property bool isDownloading: false
    property bool isInferencing: false

    // Protocol capabilities are negotiated from the bridge contract and the
    // actual in-process command transport. They are intentionally separate
    // from transient product capabilities such as canSendMessage.
    readonly property bool protocolV2Available: ContractStore.protocolV2Available
    readonly property bool authoritativeAcknowledgements: ContractStore.authoritativeAcknowledgements
    readonly property bool scopedCancellationAvailable: ContractStore.scopedCancellationAvailable
    readonly property bool eventStreamReliabilityAvailable: ContractStore.eventStreamReliabilityAvailable
    readonly property string protocolMode: ContractStore.protocolMode

    signal snapshotApplied

    function applySnapshot(capabilities) {
        if (!capabilities || typeof capabilities !== "object")
            return
        canInitialize = capabilities.can_initialize === true
        canSendMessage = capabilities.can_send_message === true
        canStopGeneration = capabilities.can_stop_generation === true
        canDownloadModel = capabilities.can_download_model === true
        canStopDownload = capabilities.can_stop_download === true
        canSwitchModel = capabilities.can_switch_model === true
        canDeleteModel = capabilities.can_delete_model === true
        canClearConversation = capabilities.can_clear_conversation === true
        canOpenSettings = capabilities.can_open_settings !== false
        needsConfig = capabilities.needs_config === true
        needsStoragePermission = capabilities.needs_storage_permission === true
        activeModelReady = capabilities.active_model_ready === true
        isBusy = capabilities.is_busy === true
        isDownloading = capabilities.is_downloading === true
        isInferencing = capabilities.is_inferencing === true
        snapshotApplied()
    }

    function applyEvent(event) {
        if (event && event.capabilities)
            applySnapshot(event.capabilities)
    }
}
