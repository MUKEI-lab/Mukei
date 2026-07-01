#include <QFontDatabase>
#include <QGuiApplication>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QOperatingSystemVersion>
#include <QQuickStyle>
#include <QQuickWindow>
#include <QSGRendererInterface>
#include <QTimer>
#include <QVariantMap>

class MukeiAgentStub final : public QObject
{
    Q_OBJECT
public:
    using QObject::QObject;
    Q_INVOKABLE bool initialize(const QString &) { return true; }
    Q_INVOKABLE void send_message(const QString &) { emit state_changed(QStringLiteral("streaming")); }
    Q_INVOKABLE void stop_generation() { emit stream_finalized(); }
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
};

class MukeiBridgeStub final : public QObject
{
    Q_OBJECT
public:
    using QObject::QObject;
    Q_INVOKABLE void set_brave_api_key(const QString &) {}
    Q_INVOKABLE void set_tavily_api_key(const QString &) {}
    Q_INVOKABLE void note_thermal_status(int status) { emit thermal_status_changed(status); }
    Q_INVOKABLE int saf_registry_count() const { return 0; }
    Q_INVOKABLE void set_model_dir(const QString &path) { m_modelDir = path; }
    Q_INVOKABLE QString model_dir() const { return m_modelDir; }
    Q_INVOKABLE QString recommended_model_id(int) const { return QStringLiteral("gemma-3-4b-q4_k_m"); }
    Q_INVOKABLE QString model_catalogue_json() const { return QStringLiteral("[]"); }
signals:
    void thermal_status_changed(int status);
    void saf_grant_revoked(const QString &token);
private:
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
    const QStringList fonts = {
        QStringLiteral(":/fonts/PlayfairDisplay-Regular.ttf"),
        QStringLiteral(":/fonts/PlayfairDisplay-Medium.ttf"),
        QStringLiteral(":/fonts/PlayfairDisplay-SemiBold.ttf"),
        QStringLiteral(":/fonts/PlayfairDisplay-Bold.ttf"),
        QStringLiteral(":/fonts/Merriweather-Regular.ttf"),
        QStringLiteral(":/fonts/Merriweather-Italic.ttf"),
        QStringLiteral(":/fonts/Merriweather-Bold.ttf"),
        QStringLiteral(":/fonts/Merriweather-BoldItalic.ttf"),
        QStringLiteral(":/fonts/Inter-Regular.ttf"),
        QStringLiteral(":/fonts/Inter-Medium.ttf"),
        QStringLiteral(":/fonts/Inter-SemiBold.ttf"),
        QStringLiteral(":/fonts/JetBrainsMono-Regular.ttf"),
        QStringLiteral(":/fonts/JetBrainsMono-Medium.ttf"),
        QStringLiteral(":/fonts/JetBrainsMono-Bold.ttf")
    };
    for (const QString &font : fonts) {
        QFontDatabase::addApplicationFont(font);
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

    MukeiAgentStub agent;
    MukeiBridgeStub bridge;
    SafRegistryStub safRegistry;

    QQmlApplicationEngine engine;
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
