#include <QObject>
#include <QQmlContext>
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

class SecurityQuickTestSetup final : public QObject {
    Q_OBJECT
public slots:
    void qmlEngineAvailable(QQmlEngine *engine)
    {
        engine->rootContext()->setContextProperty(QStringLiteral("mukeiClipboard"), &m_clipboard);
        engine->rootContext()->setContextProperty(QStringLiteral("mukeiHaptics"), &m_haptics);
    }

private:
    ClipboardStub m_clipboard;
    HapticsStub m_haptics;
};

QUICK_TEST_MAIN_WITH_SETUP(tst_Security, SecurityQuickTestSetup)

#include "tst_Security.moc"
