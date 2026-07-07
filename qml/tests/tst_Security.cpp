#include <QFile>
#include <QObject>
#include <QQmlContext>
#include <QQmlEngine>
#include <QtQuickTest/quicktest.h>

class ClipboardStub final : public QObject {
    Q_OBJECT
    Q_PROPERTY(QString text READ text NOTIFY textChanged)
public:
    Q_INVOKABLE void setText(const QString &value)
    {
        if (m_text == value) {
            return;
        }
        m_text = value;
        emit textChanged();
    }

    Q_INVOKABLE QString text() const
    {
        return m_text;
    }

signals:
    void textChanged();

private:
    QString m_text;
};

class HapticsStub final : public QObject {
    Q_OBJECT
public:
    Q_INVOKABLE void pulse(int) {}
};

class SecurityInspectorStub final : public QObject {
    Q_OBJECT
public:
    Q_INVOKABLE QString readFile(const QString &relativePath) const
    {
        QFile file(QStringLiteral(QT_TESTCASE_SOURCEDIR) + QLatin1Char('/') + relativePath);
        if (!file.open(QIODevice::ReadOnly | QIODevice::Text)) {
            return QString();
        }
        return QString::fromUtf8(file.readAll());
    }
};

class SecurityQuickTestSetup final : public QObject {
    Q_OBJECT
public slots:
    void qmlEngineAvailable(QQmlEngine *engine)
    {
        engine->rootContext()->setContextProperty(QStringLiteral("mukeiClipboard"), &m_clipboard);
        engine->rootContext()->setContextProperty(QStringLiteral("mukeiHaptics"), &m_haptics);
        engine->rootContext()->setContextProperty(QStringLiteral("securityInspector"), &m_inspector);
    }

private:
    ClipboardStub m_clipboard;
    HapticsStub m_haptics;
    SecurityInspectorStub m_inspector;
};

QUICK_TEST_MAIN_WITH_SETUP(tst_Security, SecurityQuickTestSetup)

#include "tst_Security.moc"
