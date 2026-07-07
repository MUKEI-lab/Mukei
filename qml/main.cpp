#include <QFontDatabase>
#include <QGuiApplication>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QOperatingSystemVersion>
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
#include <QGuiApplication>

#ifdef MUKEI_USE_REAL_BRIDGE
#  if __has_include("mukei_bridge/src/lib.rs.h")
#    include "mukei_bridge/src/lib.rs.h"
#  else
#    error "MUKEI_USE_REAL_BRIDGE requires the CXX-Qt generated mukei_bridge/src/lib.rs.h header in the include path"
#  endif
#endif

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

class MukeiAgentStub final : public QObject
{
    Q_OBJECT
public:
    using QObject::QObject;
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
        emit thinking_completed();
        emit stream_finalized();
        emit state_changed(QStringLiteral("idle"));
    }
    Q_INVOKABLE void download_model(const QString &, const QString &) { emit download_progress(0.0, QStringLiteral("queued")); }
    Q_INVOKABLE void stop_download() { emit download_progress(0.0, QStringLiteral("stopped")); }
    Q_INVOKABLE void clear_conversation() { emit state_changed(QStringLiteral("idle")); }
    Q_INVOKABLE QVariant get_hardware_info() const { return QVariantMap{{QStringLiteral("profile"), QStringLiteral("stub")}}; }
    Q_INVOKABLE void update_setting(const QString &, const QVariant &) {}
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
    void emitEvent(const QJsonObject &event)
    {
        emit event_emitted(QString::fromUtf8(QJsonDocument(event).toJson(QJsonDocument::Compact)));
    }

    QStringList m_chunks;
    qsizetype m_chunkIndex = 0;
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
    Q_INVOKABLE QString model_catalogue_json() const { return QStringLiteral("[]"); }
signals:
    void thermal_status_changed(int status);
    void saf_grant_revoked(const QString &token);
    void error_occurred(const QString &errorCode, const QString &message);
    void event_emitted(const QString &eventJson);
private:
    void emitEvent(const QJsonObject &event)
    {
        emit event_emitted(QString::fromUtf8(QJsonDocument(event).toJson(QJsonDocument::Compact)));
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
#ifdef Q_OS_ANDROID
    if (QOperatingSystemVersion::current() >= QOperatingSystemVersion(QOperatingSystemVersion::Android, 31)) {
        QQuickWindow::setGraphicsApi(QSGRendererInterface::Vulkan);
    } else {
        QQuickWindow::setGraphicsApi(QSGRendererInterface::OpenGL);
    }
#endif
    QQuickStyle::setStyle(QStringLiteral("Basic"));
    loadBundledFonts();

    MukeiClipboard clipboard;
#ifdef MUKEI_USE_REAL_BRIDGE
    ffi::MukeiAgent agent;
    ffi::MukeiBridge bridge;
    ffi::SafRegistry safRegistry;
#else
    MukeiAgentStub agent;
    MukeiBridgeStub bridge;
    SafRegistryStub safRegistry;
#endif

    QQmlApplicationEngine engine;
    engine.rootContext()->setContextProperty(QStringLiteral("mukeiClipboard"), &clipboard);
    engine.rootContext()->setContextProperty(QStringLiteral("mukeiAgent"), &agent);
    engine.rootContext()->setContextProperty(QStringLiteral("mukeiBridge"), &bridge);
    engine.rootContext()->setContextProperty(QStringLiteral("safRegistry"), &safRegistry);

    QObject::connect(&engine, &QQmlApplicationEngine::objectCreationFailed, &app, [] {
        QCoreApplication::exit(-1);
    }, Qt::QueuedConnection);

    QTimer::singleShot(100, &engine, [&engine] {
        engine.load(QUrl(QStringLiteral("qrc:/qt/qml/com/mukei/app/MainWindow.qml")));
    });

    return app.exec();
}

#include "main.moc"
