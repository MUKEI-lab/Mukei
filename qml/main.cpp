#include <QFontDatabase>
#include <QGuiApplication>
#include <QAccessible>
#include <QtGlobal>
#if QT_VERSION >= QT_VERSION_CHECK(6, 8, 0)
#include <QAccessibleAnnouncementEvent>
#endif
#include "timeline_model.h"
#include <QQmlApplicationEngine>
#include <QQmlError>
#include <QQmlContext>
#include <QOperatingSystemVersion>
#include <QStandardPaths>
#include <QDir>
#include <QQuickStyle>
#include <QQuickWindow>
#include <QSGRendererInterface>
#include <QStringList>
#include <QTimer>
#include <QVariantMap>
#include <QClipboard>
#include <QDateTime>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonArray>
#include <QUuid>
#include <QHash>
#include <QGuiApplication>

#ifdef MUKEI_USE_REAL_BRIDGE
#  if __has_include("mukei-bridge/src/lib.cxxqt.h")
#    include "mukei-bridge/src/lib.cxxqt.h"
#  else
#    error "MUKEI_USE_REAL_BRIDGE requires the CXX-Qt generated mukei-bridge/src/lib.cxxqt.h header in the include path"
#  endif
#endif

class MukeiRuntimeInfo final : public QObject
{
    Q_OBJECT
    Q_PROPERTY(QString configPath READ configPath CONSTANT)
    Q_PROPERTY(QString appDataPath READ appDataPath CONSTANT)
    Q_PROPERTY(QString platform READ platform CONSTANT)
    Q_PROPERTY(bool realBridge READ realBridge CONSTANT)
    Q_PROPERTY(bool autoInitialize READ autoInitialize CONSTANT)
public:
    explicit MukeiRuntimeInfo(QObject *parent = nullptr)
        : QObject(parent)
    {
        QString configRoot = QStandardPaths::writableLocation(QStandardPaths::AppConfigLocation);
        if (configRoot.isEmpty()) {
            configRoot = QStandardPaths::writableLocation(QStandardPaths::AppDataLocation);
        }
        if (configRoot.isEmpty()) {
            configRoot = QDir::homePath() + QStringLiteral("/.mukei");
        }
        QDir().mkpath(configRoot);
        m_configPath = QDir(configRoot).filePath(QStringLiteral("mukei.toml"));
        m_appDataPath = QStandardPaths::writableLocation(QStandardPaths::AppDataLocation);
        if (m_appDataPath.isEmpty()) {
            m_appDataPath = configRoot;
        }
    }

    QString configPath() const { return m_configPath; }
    QString appDataPath() const { return m_appDataPath; }
    QString platform() const
    {
#ifdef Q_OS_ANDROID
        return QStringLiteral("android");
#elif defined(Q_OS_MACOS)
        return QStringLiteral("macos");
#elif defined(Q_OS_WINDOWS)
        return QStringLiteral("windows");
#else
        return QStringLiteral("linux");
#endif
    }
    bool realBridge() const
    {
#ifdef MUKEI_USE_REAL_BRIDGE
        return true;
#else
        return false;
#endif
    }
    bool autoInitialize() const
    {
#ifdef MUKEI_USE_REAL_BRIDGE
        // Secure database bootstrap is owned by the native application runtime.
        // QML never receives or injects plaintext database key material.
        return true;
#else
        return true;
#endif
    }

private:
    QString m_configPath;
    QString m_appDataPath;
};

class MukeiClipboard final : public QObject
{
    Q_OBJECT
public:
    using QObject::QObject;
    
    Q_INVOKABLE void setText(const QString &text)
    {
        QClipboard *clipboard = QGuiApplication::clipboard();
        if (clipboard) {
            clipboard->setText(text);
        }
    }
    
    Q_INVOKABLE QString text() const
    {
        QClipboard *clipboard = QGuiApplication::clipboard();
        return clipboard ? clipboard->text() : QString();
    }
};

class MukeiAccessibilityBridge final : public QObject
{
    Q_OBJECT
    Q_PROPERTY(QString lastAnnouncement READ lastAnnouncement NOTIFY lastAnnouncementChanged)
    Q_PROPERTY(bool nativeDeliveryAvailable READ nativeDeliveryAvailable CONSTANT)
public:
    explicit MukeiAccessibilityBridge(QObject *parent = nullptr) : QObject(parent) {}

    QString lastAnnouncement() const { return m_lastAnnouncement; }
    bool nativeDeliveryAvailable() const
    {
#if QT_VERSION >= QT_VERSION_CHECK(6, 8, 0)
        return true;
#else
        return false;
#endif
    }

    Q_INVOKABLE void announce(const QString &message, bool assertive = false)
    {
        QString bounded = message.simplified();
        if (bounded.isEmpty())
            return;
        if (bounded.size() > 1000)
            bounded = bounded.left(997) + QStringLiteral("...");
        if (m_lastAnnouncement != bounded) {
            m_lastAnnouncement = bounded;
            emit lastAnnouncementChanged();
        }
#if QT_VERSION >= QT_VERSION_CHECK(6, 8, 0)
        QAccessibleAnnouncementEvent event(this, bounded);
        event.setPoliteness(assertive
            ? QAccessible::AnnouncementPoliteness::Assertive
            : QAccessible::AnnouncementPoliteness::Polite);
        QAccessible::updateAccessibility(&event);
#else
        Q_UNUSED(assertive)
#endif
        emit announcementRequested(bounded, assertive);
    }

signals:
    void lastAnnouncementChanged();
    void announcementRequested(const QString &message, bool assertive);

private:
    QString m_lastAnnouncement;
};

static QJsonObject capabilitySnapshotUninitialized()
{
    QJsonObject capabilities;
    capabilities.insert(QStringLiteral("can_initialize"), true);
    capabilities.insert(QStringLiteral("can_send_message"), false);
    capabilities.insert(QStringLiteral("can_stop_generation"), false);
    capabilities.insert(QStringLiteral("can_download_model"), false);
    capabilities.insert(QStringLiteral("can_stop_download"), false);
    capabilities.insert(QStringLiteral("can_switch_model"), false);
    capabilities.insert(QStringLiteral("can_delete_model"), false);
    capabilities.insert(QStringLiteral("can_clear_conversation"), false);
    capabilities.insert(QStringLiteral("can_open_settings"), true);
    capabilities.insert(QStringLiteral("needs_config"), false);
    capabilities.insert(QStringLiteral("needs_storage_permission"), false);
    capabilities.insert(QStringLiteral("active_model_ready"), false);
    capabilities.insert(QStringLiteral("is_busy"), false);
    capabilities.insert(QStringLiteral("is_downloading"), false);
    capabilities.insert(QStringLiteral("is_inferencing"), false);
    return capabilities;
}

static QJsonObject capabilitiesReady()
{
    QJsonObject capabilities;
    capabilities.insert(QStringLiteral("can_initialize"), false);
    capabilities.insert(QStringLiteral("can_send_message"), true);
    capabilities.insert(QStringLiteral("can_stop_generation"), false);
    capabilities.insert(QStringLiteral("can_download_model"), true);
    capabilities.insert(QStringLiteral("can_stop_download"), false);
    capabilities.insert(QStringLiteral("can_switch_model"), true);
    capabilities.insert(QStringLiteral("can_delete_model"), true);
    capabilities.insert(QStringLiteral("can_clear_conversation"), true);
    capabilities.insert(QStringLiteral("can_open_settings"), true);
    capabilities.insert(QStringLiteral("needs_config"), false);
    capabilities.insert(QStringLiteral("needs_storage_permission"), false);
    capabilities.insert(QStringLiteral("active_model_ready"), false);
    capabilities.insert(QStringLiteral("is_busy"), false);
    capabilities.insert(QStringLiteral("is_downloading"), false);
    capabilities.insert(QStringLiteral("is_inferencing"), false);
    return capabilities;
}

static QJsonObject capabilitiesDownloading()
{
    QJsonObject capabilities = capabilitiesReady();
    capabilities.insert(QStringLiteral("can_stop_download"), true);
    capabilities.insert(QStringLiteral("can_switch_model"), false);
    capabilities.insert(QStringLiteral("can_delete_model"), false);
    capabilities.insert(QStringLiteral("is_busy"), true);
    capabilities.insert(QStringLiteral("is_downloading"), true);
    return capabilities;
}

static QJsonObject capabilitiesInferencing()
{
    QJsonObject capabilities;
    capabilities.insert(QStringLiteral("can_initialize"), false);
    capabilities.insert(QStringLiteral("can_send_message"), false);
    capabilities.insert(QStringLiteral("can_stop_generation"), true);
    capabilities.insert(QStringLiteral("can_download_model"), true);
    capabilities.insert(QStringLiteral("can_stop_download"), false);
    capabilities.insert(QStringLiteral("can_switch_model"), false);
    capabilities.insert(QStringLiteral("can_delete_model"), false);
    capabilities.insert(QStringLiteral("can_clear_conversation"), false);
    capabilities.insert(QStringLiteral("can_open_settings"), true);
    capabilities.insert(QStringLiteral("needs_config"), false);
    capabilities.insert(QStringLiteral("needs_storage_permission"), false);
    capabilities.insert(QStringLiteral("active_model_ready"), false);
    capabilities.insert(QStringLiteral("is_busy"), true);
    capabilities.insert(QStringLiteral("is_downloading"), false);
    capabilities.insert(QStringLiteral("is_inferencing"), true);
    return capabilities;
}

static QJsonObject envelope(const QString &category)
{
    QJsonObject event;
    event.insert(QStringLiteral("schema_version"), 1);
    event.insert(QStringLiteral("timestamp"),
                 QDateTime::currentDateTimeUtc().toString(QStringLiteral("yyyy-MM-dd'T'HH:mm:ss.zzz'Z'")));
    event.insert(QStringLiteral("category"), category);
    return event;
}

static QJsonObject androidStorageUnknown()
{
    QJsonObject androidStorage;
    androidStorage.insert(QStringLiteral("state"), QStringLiteral("unknown"));
    return androidStorage;
}

static QJsonObject appLifecycleEvent(const QString &state, const QJsonObject &capabilities)
{
    QJsonObject event = envelope(QStringLiteral("app_lifecycle"));
    event.insert(QStringLiteral("state"), state);
    event.insert(QStringLiteral("capabilities"), capabilities);
    event.insert(QStringLiteral("android_storage"), androidStorageUnknown());
    return event;
}

static QJsonObject capabilitySnapshotEvent(const QJsonObject &capabilities)
{
    QJsonObject event = envelope(QStringLiteral("capability_snapshot"));
    event.insert(QStringLiteral("capabilities"), capabilities);
    return event;
}

static QJsonObject chatStateEvent(const QString &state, const QJsonObject &capabilities)
{
    QJsonObject event = envelope(QStringLiteral("chat_state"));
    event.insert(QStringLiteral("state"), state);
    event.insert(QStringLiteral("capabilities"), capabilities);
    return event;
}

static QJsonObject chatChunkEvent(const QString &chunk)
{
    QJsonObject event = envelope(QStringLiteral("chat_chunk"));
    event.insert(QStringLiteral("chunk"), chunk);
    return event;
}

static QJsonObject downloadStateEvent(const QString &state, const QString &modelId)
{
    QJsonObject event = envelope(QStringLiteral("download_state"));
    event.insert(QStringLiteral("state"), state);
    event.insert(QStringLiteral("model_id"), modelId);
    event.insert(QStringLiteral("destination"), QStringLiteral("model:") + modelId);
    event.insert(QStringLiteral("capabilities"),
                 state == QStringLiteral("downloading") || state == QStringLiteral("starting")
                     ? capabilitiesDownloading() : capabilitiesReady());
    return event;
}

static QJsonObject downloadProgressEvent(const QString &modelId, double progress)
{
    QJsonObject event = envelope(QStringLiteral("download_progress"));
    event.insert(QStringLiteral("state"), QStringLiteral("downloading"));
    event.insert(QStringLiteral("progress"), progress);
    event.insert(QStringLiteral("bytes_downloaded"), qint64(progress * 1000.0));
    event.insert(QStringLiteral("total_bytes"), qint64(1000));
    event.insert(QStringLiteral("model_id"), modelId);
    event.insert(QStringLiteral("destination"), QStringLiteral("model:") + modelId);
    return event;
}

static QJsonObject downloadCompletedEvent(const QString &modelId)
{
    QJsonObject event = envelope(QStringLiteral("download_completed"));
    event.insert(QStringLiteral("final_path"), QStringLiteral("model:") + modelId);
    event.insert(QStringLiteral("model_id"), modelId);
    return event;
}

class MukeiAgentStub final : public QObject
{
    Q_OBJECT
public:
    using QObject::QObject;
    Q_INVOKABLE QString submit_command_json(const QString &commandJson)
    {
        const QJsonDocument document = QJsonDocument::fromJson(commandJson.toUtf8());
        const QJsonObject command = document.isObject() ? document.object() : QJsonObject{};
        const QJsonObject version = command.value(QStringLiteral("protocol_version")).toObject();
        const QString commandId = command.value(QStringLiteral("command_id")).toString();
        const QString requestId = command.value(QStringLiteral("request_id")).toString();
        const QString correlationId = command.value(QStringLiteral("correlation_id")).toString();
        const QString commandType = command.value(QStringLiteral("command_type")).toString();
        const QJsonObject payload = command.value(QStringLiteral("payload")).toObject();
        const QJsonObject scope = command.value(QStringLiteral("scope")).toObject();
        const QStringList knownCommands = {
            QStringLiteral("app.initialize"), QStringLiteral("chat.send_message"),
            QStringLiteral("chat.stop_generation"), QStringLiteral("chat.clear_conversation"),
            QStringLiteral("model.download"), QStringLiteral("download.cancel"),
            QStringLiteral("model.select"), QStringLiteral("model.delete"),
            QStringLiteral("document.grant"), QStringLiteral("document.revoke"),
            QStringLiteral("document.retry_ingestion"), QStringLiteral("settings.update"),
            QStringLiteral("recovery.resume"), QStringLiteral("recovery.regenerate")
        };

        QString rejectionReason;
        if (version.value(QStringLiteral("major")).toInt() != 2)
            rejectionReason = QStringLiteral("unsupported_protocol");
        else if (commandId.isEmpty() || requestId.isEmpty() || correlationId.isEmpty())
            rejectionReason = QStringLiteral("invalid_payload");
        else if (!knownCommands.contains(commandType))
            rejectionReason = QStringLiteral("unknown_command");
        else if (commandType == QStringLiteral("app.initialize")
                 && payload.value(QStringLiteral("config_path")).toString().isEmpty())
            rejectionReason = QStringLiteral("invalid_payload");
        else if (commandType == QStringLiteral("chat.send_message")
                 && payload.value(QStringLiteral("text")).toString().trimmed().isEmpty())
            rejectionReason = QStringLiteral("invalid_payload");
        else if ((commandType == QStringLiteral("model.download")
                  || commandType == QStringLiteral("model.select")
                  || commandType == QStringLiteral("model.delete"))
                 && payload.value(QStringLiteral("model_id")).toString().isEmpty())
            rejectionReason = QStringLiteral("invalid_payload");
        else if (commandType == QStringLiteral("document.grant")
                 && (payload.value(QStringLiteral("target")).toString().isEmpty()
                     || payload.value(QStringLiteral("label")).toString().isEmpty()
                     || payload.value(QStringLiteral("mime_type")).toString().isEmpty()))
            rejectionReason = QStringLiteral("invalid_payload");
        else if ((commandType == QStringLiteral("document.revoke")
                  || commandType == QStringLiteral("document.retry_ingestion"))
                 && payload.value(QStringLiteral("document_id")).toString().isEmpty())
            rejectionReason = QStringLiteral("invalid_payload");
        else if (commandType == QStringLiteral("settings.update")
                 && payload.value(QStringLiteral("key")).toString().isEmpty())
            rejectionReason = QStringLiteral("invalid_payload");
        else if ((commandType == QStringLiteral("recovery.resume")
                  || commandType == QStringLiteral("recovery.regenerate"))
                 && (scope.value(QStringLiteral("conversation_id")).toString().isEmpty()
                     || scope.value(QStringLiteral("branch_id")).toString().isEmpty()))
            rejectionReason = QStringLiteral("stale_scope");

        const QString operationId = command.value(QStringLiteral("operation_id")).toString().isEmpty()
            ? QUuid::createUuid().toString(QUuid::WithoutBraces)
            : command.value(QStringLiteral("operation_id")).toString();
        QJsonObject acknowledgement;
        acknowledgement.insert(QStringLiteral("protocol_version"), QJsonObject{
            {QStringLiteral("major"), 2}, {QStringLiteral("minor"), 0}});
        acknowledgement.insert(QStringLiteral("command_id"), commandId);
        acknowledgement.insert(QStringLiteral("request_id"), requestId);
        acknowledgement.insert(QStringLiteral("correlation_id"), correlationId);
        acknowledgement.insert(QStringLiteral("timestamp"), QDateTime::currentDateTimeUtc().toString(Qt::ISODateWithMs));
        if (!rejectionReason.isEmpty()) {
            acknowledgement.insert(QStringLiteral("status"), QStringLiteral("rejected"));
            acknowledgement.insert(QStringLiteral("rejection_reason"), rejectionReason);
            return QString::fromUtf8(QJsonDocument(acknowledgement).toJson(QJsonDocument::Compact));
        }

        acknowledgement.insert(QStringLiteral("status"), QStringLiteral("accepted"));
        acknowledgement.insert(QStringLiteral("operation_id"), operationId);
        QJsonObject context;
        context.insert(QStringLiteral("command_id"), commandId);
        context.insert(QStringLiteral("request_id"), requestId);
        context.insert(QStringLiteral("correlation_id"), correlationId);
        context.insert(QStringLiteral("operation_id"), operationId);
        context.insert(QStringLiteral("command_type"), commandType);

        QTimer::singleShot(0, this, [this, commandType, payload, context]() {
            if (commandType == QStringLiteral("app.initialize")) {
                m_appContext = context;
                initialize(payload.value(QStringLiteral("config_path")).toString());
                emitOperationLifecycle(context, true, QJsonObject{
                    {QStringLiteral("initialized"), true},
                    {QStringLiteral("runtime"), QStringLiteral("stub")}
                });
                emitOperationLifecycle(context, true, QJsonObject{
                    {QStringLiteral("initialized"), true},
                    {QStringLiteral("runtime"), QStringLiteral("stub")}
                });
                emitOperationLifecycle(context, true, QJsonObject{
                    {QStringLiteral("initialized"), true},
                    {QStringLiteral("runtime"), QStringLiteral("stub")}
                });
                emitOperationLifecycle(context, true, QJsonObject{
                    {QStringLiteral("initialized"), true},
                    {QStringLiteral("runtime"), QStringLiteral("stub")}
                });
                emitOperationLifecycle(context, true, QJsonObject{
                    {QStringLiteral("initialized"), true},
                    {QStringLiteral("runtime"), QStringLiteral("stub")}
                });
            } else if (commandType == QStringLiteral("chat.send_message")) {
                m_chatContext = context;
                send_message(payload.value(QStringLiteral("text")).toString());
            } else if (commandType == QStringLiteral("chat.stop_generation")) {
                stop_generation();
                emitOperationLifecycle(context, true, QJsonObject{{QStringLiteral("cancel_requested"), true}});
            } else if (commandType == QStringLiteral("chat.clear_conversation")) {
                clear_conversation();
                emitOperationLifecycle(context, true, QJsonObject{{QStringLiteral("cleared"), true}});
            } else if (commandType == QStringLiteral("model.download")) {
                const QString modelId = payload.value(QStringLiteral("model_id")).toString();
                m_downloadContexts.insert(modelId, context);
                download_model(modelId, payload.value(QStringLiteral("sha256")).toString());
            } else if (commandType == QStringLiteral("download.cancel")) {
                stop_download();
                emitOperationLifecycle(context, true, QJsonObject{{QStringLiteral("cancel_requested"), true}});
            } else if (commandType == QStringLiteral("model.select")) {
                emitJsonOperationResult(context, select_installed_model_json(payload.value(QStringLiteral("model_id")).toString()));
            } else if (commandType == QStringLiteral("model.delete")) {
                emitJsonOperationResult(context, delete_installed_model_json(payload.value(QStringLiteral("model_id")).toString()));
            } else if (commandType == QStringLiteral("document.grant")) {
                emitJsonOperationResult(context, grant_document_access_json(
                    payload.value(QStringLiteral("target")).toString(),
                    payload.value(QStringLiteral("label")).toString(),
                    payload.value(QStringLiteral("mime_type")).toString()));
            } else if (commandType == QStringLiteral("document.revoke")) {
                emitJsonOperationResult(context, revoke_document_json(payload.value(QStringLiteral("document_id")).toString()));
            } else if (commandType == QStringLiteral("document.retry_ingestion")) {
                emitJsonOperationResult(context, retry_document_ingestion_json(payload.value(QStringLiteral("document_id")).toString()));
            } else if (commandType == QStringLiteral("settings.update")) {
                update_setting(payload.value(QStringLiteral("key")).toString(), payload.value(QStringLiteral("value")).toVariant());
                emitOperationLifecycle(context, true, QJsonObject{{QStringLiteral("key"), payload.value(QStringLiteral("key"))}});
            } else if (commandType == QStringLiteral("recovery.resume")) {
                m_chatContext = context;
                resume_interrupted_turn();
            } else if (commandType == QStringLiteral("recovery.regenerate")) {
                m_chatContext = context;
                regenerate_interrupted_turn();
            }
        });
        return QString::fromUtf8(QJsonDocument(acknowledgement).toJson(QJsonDocument::Compact));
    }

    Q_INVOKABLE bool initialize(const QString &)
    {
        emitEvent(appLifecycleEvent(QStringLiteral("booting"), capabilitySnapshotUninitialized()));
        emitEvent(appLifecycleEvent(QStringLiteral("ready"), capabilitiesReady()));
        emitEvent(capabilitySnapshotEvent(capabilitiesReady()));
        return true;
    }
    Q_INVOKABLE void send_message(const QString &message)
    {
        m_chunks = {
            QStringLiteral("Stub response for: "),
            message.left(80) + QStringLiteral(" \"quoted\"\nStreaming path is active."),
            QStringLiteral("Streaming path is active.")
        };
        m_chunkIndex = 0;
        emitEvent(chatStateEvent(QStringLiteral("submitting"), capabilitiesReady()));
        emit state_changed(QStringLiteral("streaming"));
        emit thinking_started();
        emitEvent(chatStateEvent(QStringLiteral("thinking"), capabilitiesInferencing()));
        emitEvent(chatStateEvent(QStringLiteral("streaming"), capabilitiesInferencing()));
        QTimer::singleShot(50, this, &MukeiAgentStub::emitNextChunk);
    }
    Q_INVOKABLE void stop_generation()
    {
        m_chunkIndex = m_chunks.size();
        emitEvent(envelope(QStringLiteral("chat_cancelled")));
        emitEvent(chatStateEvent(QStringLiteral("cancelled"), capabilitiesReady()));
        emit thinking_completed();
        emit stream_finalized();
        emit state_changed(QStringLiteral("idle"));
    }
    Q_INVOKABLE void download_model(const QString &modelId, const QString &)
    {
        if (!m_activeDownloadModel.isEmpty())
            return;
        m_activeDownloadModel = modelId;
        emitEvent(downloadStateEvent(QStringLiteral("starting"), modelId));
        emitEvent(downloadProgressEvent(modelId, 0.05));
        emit download_progress(0.05, QStringLiteral("downloading"));
        QTimer::singleShot(180, this, [this, modelId] {
            if (m_activeDownloadModel != modelId)
                return;
            emitEvent(downloadProgressEvent(modelId, 0.55));
            emit download_progress(0.55, QStringLiteral("downloading"));
        });
        QTimer::singleShot(360, this, [this, modelId] {
            if (m_activeDownloadModel != modelId)
                return;
            emitEvent(downloadProgressEvent(modelId, 1.0));
            emitEvent(downloadCompletedEvent(modelId));
            emitEvent(capabilitySnapshotEvent(capabilitiesReady()));
            emit download_progress(1.0, QStringLiteral("complete:model:") + modelId);
            m_activeDownloadModel.clear();
        });
    }
    Q_INVOKABLE void stop_download()
    {
        if (m_activeDownloadModel.isEmpty())
            return;
        const QString modelId = m_activeDownloadModel;
        m_activeDownloadModel.clear();
        emitEvent(downloadStateEvent(QStringLiteral("cancelled"), modelId));
        emitEvent(capabilitySnapshotEvent(capabilitiesReady()));
        emit download_progress(0.0, QStringLiteral("cancelled"));
    }
    Q_INVOKABLE void clear_conversation() { emit state_changed(QStringLiteral("idle")); }
    Q_INVOKABLE QVariant get_hardware_info() const { return QVariantMap{{QStringLiteral("profile"), QStringLiteral("stub")}}; }
    Q_INVOKABLE void update_setting(const QString &, const QVariant &) {}
    Q_INVOKABLE QString interrupted_turn_json() const { return QStringLiteral("null"); }
    Q_INVOKABLE void resume_interrupted_turn() {}
    Q_INVOKABLE void regenerate_interrupted_turn() {}
    Q_INVOKABLE QString ui_session_json() const
    {
        return m_uiSession.isEmpty()
            ? QStringLiteral("null")
            : QString::fromUtf8(QJsonDocument(m_uiSession).toJson(QJsonDocument::Compact));
    }
    Q_INVOKABLE void save_ui_session(const QString &json)
    {
        const QJsonDocument document = QJsonDocument::fromJson(json.toUtf8());
        if (document.isObject())
            m_uiSession = document.object();
    }
    Q_INVOKABLE QString draft_json(const QString &conversationId, const QString &branchId) const
    {
        const QString key = conversationId + QStringLiteral(":") + branchId;
        if (!m_drafts.contains(key))
            return QStringLiteral("null");
        QJsonObject draft;
        draft.insert(QStringLiteral("conversation_id"), conversationId);
        draft.insert(QStringLiteral("branch_id"), branchId);
        draft.insert(QStringLiteral("text"), m_drafts.value(key));
        draft.insert(QStringLiteral("cursor_position"), m_drafts.value(key).size());
        draft.insert(QStringLiteral("attachment_refs_json"), QStringLiteral("[]"));
        return QString::fromUtf8(QJsonDocument(draft).toJson(QJsonDocument::Compact));
    }
    Q_INVOKABLE void save_draft(
        const QString &conversationId,
        const QString &branchId,
        const QString &text,
        int)
    {
        m_drafts.insert(conversationId + QStringLiteral(":") + branchId, text);
    }
    Q_INVOKABLE void clear_draft(const QString &conversationId, const QString &branchId)
    {
        m_drafts.remove(conversationId + QStringLiteral(":") + branchId);
    }
    Q_INVOKABLE QString conversation_list_json(int) const { return QStringLiteral("[]"); }
    Q_INVOKABLE QString chat_snapshot_json(
        const QString &,
        const QString &,
        const QString &,
        int) const
    {
        return QStringLiteral("{\"items\":[],\"has_older\":false,\"oldest_message_id\":\"\"}");
    }
    Q_INVOKABLE QString download_jobs_json(int) const { return QStringLiteral("[]"); }
    Q_INVOKABLE QString storage_snapshot_json() const
    {
        return QStringLiteral("{\"model_bytes\":0,\"partial_bytes\":0,\"total_bytes\":0,\"accounted_model_bytes\":0,\"max_model_storage_bytes\":34359738368,\"usage_ratio\":0,\"pressure\":\"normal\"}");
    }
    Q_INVOKABLE QString document_list_json(int) const
    {
        return QString::fromUtf8(QJsonDocument(m_documents).toJson(QJsonDocument::Compact));
    }
    Q_INVOKABLE QString settings_snapshot_json() const { return QStringLiteral("[]"); }
    Q_INVOKABLE QString select_installed_model_json(const QString &modelId)
    {
        m_selectedModelId = modelId;
        QJsonObject result;
        result.insert(QStringLiteral("ok"), true);
        result.insert(QStringLiteral("model_id"), modelId);
        result.insert(QStringLiteral("state"), QStringLiteral("selected"));
        result.insert(QStringLiteral("hot_loaded"), false);
        return QString::fromUtf8(QJsonDocument(result).toJson(QJsonDocument::Compact));
    }
    Q_INVOKABLE QString delete_installed_model_json(const QString &modelId)
    {
        if (m_selectedModelId == modelId)
            m_selectedModelId.clear();
        QJsonObject result;
        result.insert(QStringLiteral("ok"), true);
        result.insert(QStringLiteral("model_id"), modelId);
        result.insert(QStringLiteral("deleted"), true);
        return QString::fromUtf8(QJsonDocument(result).toJson(QJsonDocument::Compact));
    }
    Q_INVOKABLE QString grant_document_access_json(
        const QString &target,
        const QString &label,
        const QString &mime)
    {
        Q_UNUSED(target)
        const QString documentId = QStringLiteral("doc-")
            + QUuid::createUuid().toString(QUuid::WithoutBraces).left(24);
        QJsonObject row;
        row.insert(QStringLiteral("document_id"), documentId);
        row.insert(QStringLiteral("label"), label.isEmpty() ? QStringLiteral("Private document") : label);
        row.insert(QStringLiteral("mime_type"), mime);
        row.insert(QStringLiteral("size_bytes"), 0);
        row.insert(QStringLiteral("chunk_count"), 0);
        row.insert(QStringLiteral("revoked"), false);
        row.insert(QStringLiteral("cleanup_pending"), false);
        row.insert(QStringLiteral("cleanup_attempts"), 0);
        row.insert(QStringLiteral("last_error"), QString());
        row.insert(QStringLiteral("permission_state"), QStringLiteral("not_required"));
        row.insert(QStringLiteral("ingestion_state"), QStringLiteral("waiting_for_embedder"));
        row.insert(QStringLiteral("ingestion_progress_percent"), 0);
        row.insert(QStringLiteral("ingestion_retryable"), true);
        row.insert(QStringLiteral("ingestion_error"), QJsonValue::Null);
        row.insert(QStringLiteral("updated_at"), QDateTime::currentDateTimeUtc().toString(Qt::ISODateWithMs));
        m_documents.append(row);
        QJsonObject result;
        result.insert(QStringLiteral("ok"), true);
        result.insert(QStringLiteral("document_id"), documentId);
        result.insert(QStringLiteral("state"), QStringLiteral("access_granted"));
        result.insert(QStringLiteral("permission_state"), QStringLiteral("not_required"));
        result.insert(QStringLiteral("ingestion_state"), QStringLiteral("waiting_for_embedder"));
        result.insert(QStringLiteral("indexed"), false);
        return QString::fromUtf8(QJsonDocument(result).toJson(QJsonDocument::Compact));
    }
    Q_INVOKABLE QString revoke_document_json(const QString &documentId)
    {
        for (qsizetype i = 0; i < m_documents.size(); ++i) {
            QJsonObject row = m_documents.at(i).toObject();
            if (row.value(QStringLiteral("document_id")).toString() == documentId) {
                row.insert(QStringLiteral("revoked"), true);
                row.insert(QStringLiteral("cleanup_pending"), false);
                m_documents.replace(i, row);
                break;
            }
        }
        QJsonObject result;
        result.insert(QStringLiteral("ok"), true);
        result.insert(QStringLiteral("document_id"), documentId);
        result.insert(QStringLiteral("revoked"), true);
        return QString::fromUtf8(QJsonDocument(result).toJson(QJsonDocument::Compact));
    }
    Q_INVOKABLE QString retry_document_ingestion_json(const QString &documentId)
    {
        for (qsizetype i = 0; i < m_documents.size(); ++i) {
            QJsonObject row = m_documents.at(i).toObject();
            if (row.value(QStringLiteral("document_id")).toString() == documentId) {
                row.insert(QStringLiteral("ingestion_state"), QStringLiteral("waiting_for_embedder"));
                row.insert(QStringLiteral("ingestion_progress_percent"), 0);
                row.insert(QStringLiteral("ingestion_retryable"), true);
                row.insert(QStringLiteral("ingestion_error"), QJsonValue::Null);
                m_documents.replace(i, row);
                break;
            }
        }
        QJsonObject result;
        result.insert(QStringLiteral("ok"), true);
        result.insert(QStringLiteral("document_id"), documentId);
        result.insert(QStringLiteral("ingestion_state"), QStringLiteral("waiting_for_embedder"));
        result.insert(QStringLiteral("indexed"), false);
        return QString::fromUtf8(QJsonDocument(result).toJson(QJsonDocument::Compact));
    }
    Q_INVOKABLE QString ui_contract_snapshot_json() const
    {
        return QStringLiteral(R"json({"schema_version":1,"contract_version":1,"min_qml_contract_version":1,"max_qml_contract_version":1,"command_schema_version":2,"event_schema_version":1,"snapshot_schema_version":1,"required_features":["typed_commands","typed_events","snapshot_delta_sync","persistent_ui_session","capability_gating","command_envelope_v2","command_acknowledgement","operation_lifecycle_events","scoped_chat_operations","legacy_event_v1_compatibility"],"protocol":{"current_version":{"major":2,"minor":0},"minimum_supported_peer_major":2,"capabilities":["command_envelope_v2","command_acknowledgement","operation_lifecycle_events","scoped_chat_operations","legacy_event_v1_compatibility"]}})json");
    }
    Q_INVOKABLE QString operation_snapshot_json() const
    {
        QJsonArray operations;
        for (const QJsonValue &value : m_documents) {
            const QJsonObject document = value.toObject();
            const QString state = document.value(QStringLiteral("ingestion_state")).toString();
            if (state == QStringLiteral("waiting_for_embedder")) {
                QJsonObject operation;
                operation.insert(QStringLiteral("operation_id"), QStringLiteral("document_ingestion:") + document.value(QStringLiteral("document_id")).toString());
                operation.insert(QStringLiteral("type"), QStringLiteral("document_ingestion"));
                operation.insert(QStringLiteral("state"), QStringLiteral("blocked"));
                operation.insert(QStringLiteral("phase"), state);
                operation.insert(QStringLiteral("progress"), 0.0);
                operation.insert(QStringLiteral("cancelable"), false);
                operation.insert(QStringLiteral("retryable"), true);
                operation.insert(QStringLiteral("label"), QStringLiteral("Waiting for document embedder"));
                operation.insert(QStringLiteral("related_entity_id"), document.value(QStringLiteral("document_id")).toString());
                operations.append(operation);
            }
        }
        QJsonObject result;
        result.insert(QStringLiteral("schema_version"), 1);
        result.insert(QStringLiteral("operations"), operations);
        return QString::fromUtf8(QJsonDocument(result).toJson(QJsonDocument::Compact));
    }
    Q_INVOKABLE QString engine_session_snapshot_json() const
    {
        QJsonObject result;
        result.insert(QStringLiteral("schema_version"), 1);
        result.insert(QStringLiteral("selected_model_id"), m_selectedModelId);
        result.insert(QStringLiteral("loaded_model_id"), QJsonValue::Null);
        result.insert(QStringLiteral("inference_backend"), QStringLiteral("stub"));
        result.insert(QStringLiteral("activation_supported"), false);
        result.insert(QStringLiteral("restart_required"), !m_selectedModelId.isEmpty());
        result.insert(QStringLiteral("safe_message"), QStringLiteral("Demo bridge does not activate a native model."));
        return QString::fromUtf8(QJsonDocument(result).toJson(QJsonDocument::Compact));
    }
    Q_INVOKABLE QString diagnostics_snapshot_json() const
    {
        QJsonObject privacy;
        privacy.insert(QStringLiteral("contains_prompts"), false);
        privacy.insert(QStringLiteral("contains_document_contents"), false);
        privacy.insert(QStringLiteral("contains_secrets"), false);
        privacy.insert(QStringLiteral("contains_private_paths"), false);
        QJsonObject result;
        result.insert(QStringLiteral("schema_version"), 1);
        result.insert(QStringLiteral("runtime_phase"), QStringLiteral("ReadyStub"));
        result.insert(QStringLiteral("ready"), true);
        result.insert(QStringLiteral("document_grant_count"), m_documents.size());
        result.insert(QStringLiteral("privacy"), privacy);
        return QString::fromUtf8(QJsonDocument(result).toJson(QJsonDocument::Compact));
    }
    Q_INVOKABLE QString provenance_snapshot_json() const
    {
        QJsonObject result;
        result.insert(QStringLiteral("schema_version"), 1);
        result.insert(QStringLiteral("product_version"), QString::fromLatin1(MUKEI_PRODUCT_VERSION));
        result.insert(QStringLiteral("protocol_version"), 1);
        result.insert(QStringLiteral("database_schema_version"), 0);
        result.insert(QStringLiteral("build_identifier"), QJsonValue::Null);
        result.insert(QStringLiteral("compiler_profile"), QStringLiteral("stub"));
        result.insert(QStringLiteral("runtime_environment_mode"), QStringLiteral("development"));
        result.insert(QStringLiteral("hardening_mode"), QStringLiteral("standard"));
        result.insert(QStringLiteral("feature_flags"), QJsonArray{});
        return QString::fromUtf8(QJsonDocument(result).toJson(QJsonDocument::Compact));
    }
    Q_INVOKABLE QString export_diagnostics_json() const
    {
        QJsonObject result;
        result.insert(QStringLiteral("ok"), true);
        result.insert(QStringLiteral("export_id"), QStringLiteral("diagnostics-stub"));
        result.insert(QStringLiteral("filename"), QStringLiteral("diagnostics-stub.json"));
        return QString::fromUtf8(QJsonDocument(result).toJson(QJsonDocument::Compact));
    }
signals:
    void chunk_generated(const QString &chunk);
    void stream_finalized();
    void state_changed(const QString &state);
    void tool_call_started(const QString &toolName);
    void tool_call_completed(const QString &toolName, const QString &result);
    void error_occurred(const QString &errorCode, const QString &message);
    void download_progress(double progress, const QString &status);
    void thinking_started();
    void thinking_completed();
    void event_emitted(const QString &eventJson);
    void eventEmitted(const QString &eventJson);
    void async_result(const QString &resultJson);
private slots:
    void emitNextChunk()
    {
        if (m_chunkIndex >= m_chunks.size()) {
            emitEvent(envelope(QStringLiteral("chat_completed")));
            emitEvent(chatStateEvent(QStringLiteral("completed"), capabilitiesReady()));
            emitEvent(capabilitySnapshotEvent(capabilitiesReady()));
            emit thinking_completed();
            emit stream_finalized();
            emit state_changed(QStringLiteral("idle"));
            return;
        }
        const QString chunk = m_chunks.at(m_chunkIndex++);
        emit chunk_generated(chunk);
        emitEvent(chatChunkEvent(chunk));
        QTimer::singleShot(50, this, &MukeiAgentStub::emitNextChunk);
    }
private:
    void emitOperationLifecycle(const QJsonObject &context, bool ok, const QJsonObject &payload)
    {
        QJsonObject event = envelope(QStringLiteral("operation_lifecycle"));
        for (auto it = context.begin(); it != context.end(); ++it)
            event.insert(it.key(), it.value());
        event.insert(QStringLiteral("state"), ok ? QStringLiteral("completed") : QStringLiteral("failed"));
        if (ok)
            event.insert(QStringLiteral("result"), payload);
        else
            event.insert(QStringLiteral("error"), payload);
        emitEvent(event);
    }

    void emitJsonOperationResult(const QJsonObject &context, const QString &raw)
    {
        const QJsonDocument resultDocument = QJsonDocument::fromJson(raw.toUtf8());
        const QJsonObject result = resultDocument.isObject() ? resultDocument.object() : QJsonObject{};
        const bool ok = result.value(QStringLiteral("ok")).toBool(false);
        emitOperationLifecycle(context, ok,
            ok ? result : result.value(QStringLiteral("error")).toObject());
    }

    void emitEvent(const QJsonObject &sourceEvent)
    {
        QJsonObject event = sourceEvent;
        const QString category = event.value(QStringLiteral("category")).toString();
        QJsonObject context;
        if (category == QStringLiteral("app_lifecycle")) {
            context = m_appContext;
        } else if (category.startsWith(QStringLiteral("chat_"))) {
            context = m_chatContext;
        } else if (category.startsWith(QStringLiteral("download_"))) {
            const QString modelId = event.value(QStringLiteral("model_id")).toString();
            context = m_downloadContexts.value(modelId);
        }
        for (auto it = context.begin(); it != context.end(); ++it)
            if (!event.contains(it.key()))
                event.insert(it.key(), it.value());
        const QString eventJson = QString::fromUtf8(
            QJsonDocument(event).toJson(QJsonDocument::Compact));
        emit eventEmitted(eventJson);

        if (category == QStringLiteral("app_lifecycle")) {
            const QString state = event.value(QStringLiteral("state")).toString();
            if (state == QStringLiteral("ready") || state == QStringLiteral("degraded") || state == QStringLiteral("fatal_error"))
                m_appContext = QJsonObject{};
        } else if (category == QStringLiteral("chat_completed") || category == QStringLiteral("chat_cancelled")
                   || (category == QStringLiteral("chat_state") && (event.value(QStringLiteral("state")).toString() == QStringLiteral("failed")
                       || event.value(QStringLiteral("state")).toString() == QStringLiteral("completed")
                       || event.value(QStringLiteral("state")).toString() == QStringLiteral("cancelled")))) {
            m_chatContext = QJsonObject{};
        } else if (category == QStringLiteral("download_completed")
                   || (category == QStringLiteral("download_state") && (event.value(QStringLiteral("state")).toString() == QStringLiteral("failed")
                       || event.value(QStringLiteral("state")).toString() == QStringLiteral("completed")
                       || event.value(QStringLiteral("state")).toString() == QStringLiteral("cancelled")))) {
            m_downloadContexts.remove(event.value(QStringLiteral("model_id")).toString());
        }
    }

    QJsonObject m_appContext;
    QJsonObject m_chatContext;
    QHash<QString, QJsonObject> m_downloadContexts;
    QString m_activeDownloadModel;
    QStringList m_chunks;
    qsizetype m_chunkIndex = 0;
    QJsonObject m_uiSession;
    QHash<QString, QString> m_drafts;
    QJsonArray m_documents;
    QString m_selectedModelId;
};

class MukeiBridgeStub final : public QObject
{
    Q_OBJECT
public:
    using QObject::QObject;
    Q_INVOKABLE void set_brave_api_key(const QString &) {}
    Q_INVOKABLE void set_tavily_api_key(const QString &) {}
    Q_INVOKABLE void set_database_cipher_key(const QString &) {}
    Q_INVOKABLE void note_thermal_status(int status)
    {
        emit thermal_status_changed(status);
        emitEvent(capabilitySnapshotEvent(status > 0 ? capabilitiesInferencing() : capabilitiesReady()));
    }
    Q_INVOKABLE int saf_registry_count() const { return 0; }
    Q_INVOKABLE void set_model_dir(const QString &path) { m_modelDir = path; }
    Q_INVOKABLE QString model_dir() const { return m_modelDir; }
    Q_INVOKABLE QString recommended_model_id(int) const { return QStringLiteral("gemma-3-4b-q4_k_m"); }
    Q_INVOKABLE QString model_catalogue_json() const
    {
        return QStringLiteral(R"json([
            {"id":"gemma-4-e2b-it","display_name":"Gemma 4 E2B Instruct (Q4_K_M)","description":"Balanced local model for mid-tier phones.","approximate_bytes":3462678272,"min_device_ram_mib":4096,"recommended_n_ctx":4096,"filename":"google_gemma-4-E2B-it-Q4_K_M.gguf","installed":false,"bytes_on_disk":0},
            {"id":"gemma-4-e4b-it","display_name":"Gemma 4 E4B Instruct (Q4_K_M)","description":"Higher-capability local model for flagship phones.","approximate_bytes":5405168384,"min_device_ram_mib":7168,"recommended_n_ctx":8192,"filename":"google_gemma-4-E4B-it-Q4_K_M.gguf","installed":false,"bytes_on_disk":0}
        ])json");
    }
signals:
    void thermal_status_changed(int status);
    void saf_grant_revoked(const QString &token);
    void error_occurred(const QString &errorCode, const QString &message);
    void event_emitted(const QString &eventJson);
private:
    void emitEvent(const QJsonObject &event)
    {
        const QString eventJson = QString::fromUtf8(
            QJsonDocument(event).toJson(QJsonDocument::Compact));
        emit eventEmitted(eventJson);
    }

    QString m_modelDir;
};

class SafRegistryStub final : public QObject
{
    Q_OBJECT
public:
    using QObject::QObject;
    Q_INVOKABLE bool upsert_grant(const QString &, const QString &, const QString &) { return true; }
    Q_INVOKABLE QString resolve_token(const QString &) const { return QString(); }
    Q_INVOKABLE bool revoke_token(const QString &token) { emit token_revoked(token); return true; }
    Q_INVOKABLE int count() const { return 0; }
signals:
    void token_revoked(const QString &token);
};

static void loadBundledFonts()
{
    // Variable-axis SIL OFL fonts. Each `wght` axis exposes every weight
    // the UX Brief references (Regular, Medium, SemiBold, Bold), so QML
    // only sees four family names — the actual weight is picked at render
    // time via `Font.weight`. Failure to load is not fatal; Qt falls back
    // to the platform default and QFontDatabase logs a warning we surface
    // through diagnostics.
    const QStringList fonts = {
        QStringLiteral(":/fonts/PlayfairDisplay-Variable.ttf"),
        QStringLiteral(":/fonts/PlayfairDisplay-Italic-Variable.ttf"),
        QStringLiteral(":/fonts/Merriweather-Variable.ttf"),
        QStringLiteral(":/fonts/Merriweather-Italic-Variable.ttf"),
        QStringLiteral(":/fonts/Inter-Variable.ttf"),
        QStringLiteral(":/fonts/Inter-Italic-Variable.ttf"),
        QStringLiteral(":/fonts/JetBrainsMono-Variable.ttf"),
        QStringLiteral(":/fonts/JetBrainsMono-Italic-Variable.ttf")
    };
    for (const QString &font : fonts) {
        const int id = QFontDatabase::addApplicationFont(font);
        if (id < 0) {
            qWarning("MukeiFonts: failed to register bundled font '%s' — falling back to system default",
                     qUtf8Printable(font));
        }
    }
}

int main(int argc, char *argv[])
{
    QGuiApplication app(argc, argv);
    QCoreApplication::setOrganizationName(QStringLiteral("MUKEI-lab"));
    QCoreApplication::setOrganizationDomain(QStringLiteral("mukei.app"));
    QCoreApplication::setApplicationName(QStringLiteral("Mukei"));
    QCoreApplication::setApplicationVersion(QString::fromLatin1(MUKEI_PRODUCT_VERSION));
#ifdef Q_OS_ANDROID
    if (QOperatingSystemVersion::current() >= QOperatingSystemVersion(QOperatingSystemVersion::Android, 31)) {
        QQuickWindow::setGraphicsApi(QSGRendererInterface::OpenGL);
    } else {
        QQuickWindow::setGraphicsApi(QSGRendererInterface::OpenGL);
    }
#endif
    QQuickStyle::setStyle(QStringLiteral("Basic"));
    loadBundledFonts();

    MukeiClipboard clipboard;
    MukeiAccessibilityBridge accessibilityBridge;
    MukeiRuntimeInfo runtimeInfo;
    MukeiTimelineModel timelineModel;
#ifdef MUKEI_USE_REAL_BRIDGE
    MukeiAgent agent;
    MukeiBridge bridge;
    SafRegistry safRegistry;
#else
    MukeiAgentStub agent;
    MukeiBridgeStub bridge;
    SafRegistryStub safRegistry;
#endif

    const QString modelsPath = QDir(runtimeInfo.appDataPath()).filePath(QStringLiteral("models"));
    QDir().mkpath(modelsPath);
    bridge.set_model_dir(modelsPath);

    QQmlApplicationEngine engine;
    engine.rootContext()->setContextProperty(QStringLiteral("mukeiClipboard"), &clipboard);
    engine.rootContext()->setContextProperty(QStringLiteral("mukeiAccessibility"), &accessibilityBridge);
    engine.rootContext()->setContextProperty(QStringLiteral("mukeiRuntime"), &runtimeInfo);
    engine.rootContext()->setContextProperty(QStringLiteral("mukeiTimelineModel"), &timelineModel);
    engine.rootContext()->setContextProperty(QStringLiteral("mukeiAgent"), &agent);
    engine.rootContext()->setContextProperty(QStringLiteral("mukeiBridge"), &bridge);
    engine.rootContext()->setContextProperty(QStringLiteral("safRegistry"), &safRegistry);

    const QUrl mainWindowUrl(QStringLiteral("qrc:/com/mukei/app/MainWindow.qml"));
    QObject::connect(&engine, &QQmlApplicationEngine::warnings, &app,
                     [](const QList<QQmlError> &warnings) {
        for (const QQmlError &warning : warnings)
            qCritical().noquote() << "MukeiQml" << warning.toString();
    });
    QObject::connect(&engine, &QQmlApplicationEngine::objectCreated, &app,
                     [mainWindowUrl](QObject *object, const QUrl &url) {
        qInfo().noquote() << "MukeiStartup root_object"
                          << (object ? "ready" : "failed")
                          << url.toString();
        if (!object && url == mainWindowUrl)
            QCoreApplication::exit(EXIT_FAILURE);
    }, Qt::QueuedConnection);
    engine.load(mainWindowUrl);

    return app.exec();
}

#include "main.moc"
