import QtQuick
import QtTest
import "../stores"

TestCase {
    name: "ModelStoreActivation"

    function init() {
        ModelStore.models.clear()
        ModelStore.models.append({ modelId: "model-old", installed: true })
        ModelStore.models.append({ modelId: "model-new", installed: true })
        ModelStore.models.append({ modelId: "model-a", installed: true })
        ModelStore.models.append({ modelId: "model-b", installed: true })
        ModelStore.activeModelId = "model-old"
        ModelStore.loadedModelId = "model-old"
        ModelStore.selectedModelId = "model-old"
        ModelStore.activationInProgress = false
        ModelStore.activationFailed = false
        ModelStore.activationRequestId = ""
        ModelStore.activationOperationId = ""
        ModelStore.activationModelId = ""
    }

    function test_acceptance_does_not_claim_new_backend_active() {
        verify(ModelStore.beginActivation("model-new", "request-1", "operation-1"))
        compare(ModelStore.selectedModelId, "model-new")
        compare(ModelStore.activeModelId, "model-old")
        verify(ModelStore.activationInProgress)
    }

    function test_correlated_protocol_completion_publishes_active_backend() {
        ModelStore.beginActivation("model-new", "", "operation-1")
        ModelStore.applyEvent({
            category: "operation_lifecycle",
            command_type: "model.select",
            operation_id: "operation-1",
            state: "completed",
            result: {
                model_id: "model-new",
                active_model_id: "model-new",
                inference_backend: "llama_cpp_native",
                backend_kind: "production",
                active_model_ready: true,
                product_ready: true
            }
        })
        compare(ModelStore.activeModelId, "model-new")
        verify(ModelStore.activeModelReady)
        verify(!ModelStore.activationInProgress)
    }

    function test_stale_completion_cannot_replace_newer_activation() {
        ModelStore.beginActivation("model-a", "", "operation-a")
        ModelStore.beginActivation("model-b", "", "operation-b")
        ModelStore.applyEvent({
            category: "operation_lifecycle",
            command_type: "model.select",
            operation_id: "operation-a",
            state: "completed",
            result: { model_id: "model-a", active_model_id: "model-a" }
        })
        compare(ModelStore.activeModelId, "model-old")
        compare(ModelStore.activationModelId, "model-b")
        verify(ModelStore.activationInProgress)
    }

    function test_failed_replacement_preserves_previous_active_backend() {
        ModelStore.beginActivation("model-new", "", "operation-1")
        ModelStore.applyEvent({
            category: "operation_lifecycle",
            command_type: "model.select",
            operation_id: "operation-1",
            state: "failed",
            error: { code: "ERR_MODEL_LOAD" }
        })
        compare(ModelStore.activeModelId, "model-old")
        verify(ModelStore.activationFailed)
        verify(!ModelStore.activationInProgress)
    }
}
