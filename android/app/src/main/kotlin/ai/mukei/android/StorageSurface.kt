package ai.mukei.android

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import ai.mukei.android.designsystem.MukeiLayout
import ai.mukei.android.designsystem.MukeiRadius
import ai.mukei.android.designsystem.MukeiSpacing
import ai.mukei.android.designsystem.MukeiStroke
import java.util.UUID
import org.json.JSONArray
import org.json.JSONObject

private enum class StorageRowState {
    IMPORTING,
    READY,
    FAILED,
}

private data class StorageRow(
    val rowId: String,
    val operationId: String?,
    val nodeId: String?,
    val name: String,
    val sizeBytes: Long?,
    val deduplicated: Boolean,
    val state: StorageRowState,
    val detail: String? = null,
)

private data class SelectedStorageDocument(
    val uri: Uri,
    val displayName: String,
    val mimeType: String,
)

@Composable
internal fun StorageSurface() {
    val context = LocalContext.current
    val rows = remember { mutableStateListOf<StorageRow>() }
    val pendingNames = remember { mutableStateMapOf<String, String>() }
    val workspaceConversationId = remember(context) { storageInboxConversationId(context) }
    var banner by remember { mutableStateOf<String?>(null) }

    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        val selected = resolveSelectedDocument(context, uri)
        if (selected == null) {
            banner = "The selected file could not be inspected safely."
            return@rememberLauncherForActivityResult
        }
        runCatching {
            context.contentResolver.takePersistableUriPermission(
                uri,
                Intent.FLAG_GRANT_READ_URI_PERMISSION,
            )
        }
        val submission = BackendRuntimeHost.submitStorageImport(
            conversationId = workspaceConversationId,
            target = selected.uri.toString(),
            displayName = selected.displayName,
            mimeType = selected.mimeType,
        )
        if (submission.status != "accepted" || submission.operationId.isNullOrBlank()) {
            banner = friendlyStorageFailure(submission.rejectionReason ?: "storage_import_rejected")
            return@rememberLauncherForActivityResult
        }
        val operationId = submission.operationId
        pendingNames[operationId] = selected.displayName
        rows.removeAll { it.operationId == operationId }
        rows.add(
            0,
            StorageRow(
                rowId = "operation:$operationId",
                operationId = operationId,
                nodeId = null,
                name = selected.displayName,
                sizeBytes = null,
                deduplicated = false,
                state = StorageRowState.IMPORTING,
            ),
        )
        banner = null
    }

    LaunchedEffect(Unit) {
        val persisted = parseStorageHistory(BackendRuntimeHost.requestRuntimeSnapshot("operations"))
        if (persisted.isNotEmpty()) {
            rows.clear()
            rows.addAll(persisted)
        }
    }

    DisposableEffect(Unit) {
        val registration = BackendRuntimeHost.addEventListener { batch ->
            batch.events.forEach { raw ->
                runCatching { JSONObject(raw) }.getOrNull()?.let { event ->
                    val operationId = event.optString("operation_id").takeIf(String::isNotBlank)
                    when (event.optString("event_type")) {
                        "storage.file_imported" -> {
                            val payload = event.optJSONObject("payload") ?: return@let
                            val nodeId = payload.optString("node_id").takeIf(String::isNotBlank)
                            val name = payload.optString("display_name").ifBlank {
                                operationId?.let(pendingNames::get).orEmpty()
                            }.ifBlank { "Imported file" }
                            if (operationId != null) pendingNames.remove(operationId)
                            rows.removeAll {
                                (operationId != null && it.operationId == operationId) ||
                                    (nodeId != null && it.nodeId == nodeId)
                            }
                            rows.add(
                                0,
                                StorageRow(
                                    rowId = nodeId ?: "operation:${operationId ?: UUID.randomUUID()}",
                                    operationId = operationId,
                                    nodeId = nodeId,
                                    name = name,
                                    sizeBytes = payload.optLong("size_bytes").takeIf { payload.has("size_bytes") },
                                    deduplicated = payload.optBoolean("deduplicated", false),
                                    state = StorageRowState.READY,
                                    detail = payload.optString("ocr_status").takeIf {
                                        it.isNotBlank() && it != "unavailable"
                                    }?.let { "OCR: $it" },
                                ),
                            )
                            banner = "Imported securely."
                        }

                        "operation.failed" -> {
                            if (operationId == null || !pendingNames.containsKey(operationId)) return@let
                            val payload = event.optJSONObject("payload")
                            val code = payload?.optString("code")?.takeIf(String::isNotBlank)
                                ?: "storage_import_failed"
                            val name = pendingNames.remove(operationId) ?: "Selected file"
                            rows.removeAll { it.operationId == operationId }
                            rows.add(
                                0,
                                StorageRow(
                                    rowId = "operation:$operationId",
                                    operationId = operationId,
                                    nodeId = null,
                                    name = name,
                                    sizeBytes = null,
                                    deduplicated = false,
                                    state = StorageRowState.FAILED,
                                    detail = friendlyStorageFailure(code),
                                ),
                            )
                            banner = friendlyStorageFailure(code)
                        }
                    }
                }
            }
        }
        onDispose { registration.close() }
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(MukeiLayout.LargePhoneTextPadding),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .widthIn(max = MukeiLayout.ReadableContentMaxWidth),
        ) {
            Text(
                text = "Storage",
                style = MaterialTheme.typography.headlineMedium,
            )
            Spacer(Modifier.height(MukeiSpacing.ExtraSmall))
            Text(
                text = "Files are copied through Android’s document broker, validated, then committed to Mukei’s encrypted object store.",
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(MukeiSpacing.Large))

            Button(
                onClick = { picker.launch(arrayOf("*/*")) },
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text("Import file")
            }

            Spacer(Modifier.height(MukeiSpacing.Small))
            Text(
                text = "Phase 1: UTF-8 text/source files up to 32 MiB. PDF, DOCX, PNG and other binary formats are rejected safely.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )

            banner?.let { message ->
                Spacer(Modifier.height(MukeiSpacing.Medium))
                Surface(
                    modifier = Modifier.fillMaxWidth(),
                    shape = RoundedCornerShape(MukeiRadius.Composer),
                    color = MaterialTheme.colorScheme.surfaceVariant,
                ) {
                    Text(
                        text = message,
                        modifier = Modifier.padding(MukeiSpacing.Medium),
                        style = MaterialTheme.typography.bodyMedium,
                    )
                }
            }

            Spacer(Modifier.height(MukeiSpacing.Large))
            Text(
                text = if (rows.isEmpty()) "No imported files yet" else "Imported files",
                style = MaterialTheme.typography.titleMedium,
            )
            Spacer(Modifier.height(MukeiSpacing.Small))

            if (rows.isEmpty()) {
                Surface(
                    modifier = Modifier.fillMaxWidth(),
                    shape = MaterialTheme.shapes.large,
                    color = MaterialTheme.colorScheme.surface,
                    border = BorderStroke(MukeiStroke.Thin, MaterialTheme.colorScheme.outline),
                ) {
                    Text(
                        text = "Choose a supported file to create your encrypted Storage Inbox workspace.",
                        modifier = Modifier.padding(MukeiSpacing.Large),
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            } else {
                Column(verticalArrangement = Arrangement.spacedBy(MukeiSpacing.Small)) {
                    rows.forEach { row -> StorageRowCard(row) }
                }
            }
            Spacer(Modifier.height(MukeiSpacing.Major))
        }
    }
}

@Composable
private fun StorageRowCard(row: StorageRow) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = MaterialTheme.shapes.large,
        color = MaterialTheme.colorScheme.surface,
        border = BorderStroke(MukeiStroke.Thin, MaterialTheme.colorScheme.outline),
    ) {
        Row(
            modifier = Modifier.padding(MukeiSpacing.Medium),
            horizontalArrangement = Arrangement.spacedBy(MukeiSpacing.Medium),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (row.state == StorageRowState.IMPORTING) {
                CircularProgressIndicator()
            }
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = row.name,
                    style = MaterialTheme.typography.titleMedium,
                )
                val summary = when (row.state) {
                    StorageRowState.IMPORTING -> "Encrypting and committing…"
                    StorageRowState.READY -> buildString {
                        append(row.sizeBytes?.let(::formatBytes) ?: "Stored")
                        if (row.deduplicated) append(" · deduplicated")
                    }
                    StorageRowState.FAILED -> row.detail ?: "Import failed safely"
                }
                Text(
                    text = summary,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                row.detail?.takeIf { row.state == StorageRowState.READY }?.let { detail ->
                    Text(
                        text = detail,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
}

private fun resolveSelectedDocument(context: Context, uri: Uri): SelectedStorageDocument? {
    val displayName = runCatching {
        context.contentResolver.query(
            uri,
            arrayOf(OpenableColumns.DISPLAY_NAME),
            null,
            null,
            null,
        )?.use { cursor ->
            if (!cursor.moveToFirst()) return@use null
            val index = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
            if (index < 0) null else cursor.getString(index)
        }
    }.getOrNull()?.trim().orEmpty()
    if (displayName.isBlank() || displayName.length > 255) return null
    val mimeType = context.contentResolver.getType(uri)
        ?.trim()
        ?.takeIf(String::isNotBlank)
        ?: "application/octet-stream"
    return SelectedStorageDocument(uri, displayName, mimeType)
}

private fun storageInboxConversationId(context: Context): String {
    val preferences = context.getSharedPreferences("mukei.storage", Context.MODE_PRIVATE)
    preferences.getString("storage_inbox_conversation_id", null)?.let { existing ->
        runCatching { UUID.fromString(existing) }.getOrNull()?.let { return existing }
    }
    val created = UUID.randomUUID().toString()
    preferences.edit().putString("storage_inbox_conversation_id", created).apply()
    return created
}

private fun parseStorageHistory(raw: String?): List<StorageRow> {
    if (raw.isNullOrBlank()) return emptyList()
    return runCatching {
        val envelope = JSONObject(raw)
        val payload = envelope.opt("payload")
        val operations = when (payload) {
            is JSONArray -> payload
            is JSONObject -> payload.optJSONArray("operations") ?: JSONArray()
            else -> JSONArray()
        }
        buildList {
            for (index in 0 until operations.length()) {
                val operation = operations.optJSONObject(index) ?: continue
                if (operation.optString("command_type") != "storage.import_file") continue
                if (operation.optString("status") != "completed") continue
                val result = operation.optJSONObject("result") ?: continue
                val nodeId = result.optString("node_id").takeIf(String::isNotBlank) ?: continue
                add(
                    StorageRow(
                        rowId = nodeId,
                        operationId = operation.optString("operation_id").takeIf(String::isNotBlank),
                        nodeId = nodeId,
                        name = result.optString("display_name").ifBlank { "Imported file" },
                        sizeBytes = result.optLong("size_bytes").takeIf { result.has("size_bytes") },
                        deduplicated = result.optBoolean("deduplicated", false),
                        state = StorageRowState.READY,
                    ),
                )
            }
        }.reversed()
    }.getOrDefault(emptyList())
}

private fun friendlyStorageFailure(code: String): String = when (code) {
    "file_policy_rejected" -> "That filename or file type is not supported by the Phase 1 storage policy."
    "staged_file_too_large", "document_too_large" -> "That file is larger than the 32 MiB import limit."
    "non_utf8_text_rejected" -> "This Phase 1 importer accepts UTF-8 text/source files only."
    "storage_import_cancelled" -> "The import was cancelled before publication."
    "android_permission_denied" -> "Android did not grant permission to read that file."
    "document_open_failed", "platform_document_stage_failed" -> "The selected document could not be staged safely."
    "storage_import_commit_failed", "encrypted_object_store_failed" -> "The encrypted storage commit failed; no partial file was published."
    "backend_unavailable" -> "The secure runtime is not available."
    else -> "Import failed safely ($code)."
}

private fun formatBytes(value: Long): String = when {
    value < 1_024L -> "$value B"
    value < 1_048_576L -> String.format("%.1f KiB", value / 1_024.0)
    else -> String.format("%.1f MiB", value / 1_048_576.0)
}
