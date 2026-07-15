#!/usr/bin/env python3
"""Replace the Android entrypoint with a minimal runtime probe.

The probes deliberately avoid the Mukei bridge, database, model runtime, and
packaged MainWindow.qml so a physical-device launch can isolate the failing
layer without adb access.
"""
from __future__ import annotations

import argparse
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MAIN = ROOT / "qml/main.cpp"
CMAKE = ROOT / "qml/CMakeLists.txt"
MANIFEST = ROOT / "qml/android/AndroidManifest.xml"

WIDGETS_SOURCE = r'''#include <QApplication>
#include <QLabel>
#include <QVBoxLayout>
#include <QWidget>

int main(int argc, char *argv[])
{
    qputenv("QT_LOGGING_RULES", QByteArrayLiteral("*.debug=true;qt.*=true"));
    qInfo("MukeiProbe stage=process_start variant=qt-widgets-raster");

    QApplication app(argc, argv);
    app.setApplicationName(QStringLiteral("Mukei Qt Probe"));
    qInfo("MukeiProbe stage=qapplication_ready variant=qt-widgets-raster");

    QWidget window;
    window.setWindowTitle(QStringLiteral("Mukei Qt Probe"));
    window.setStyleSheet(QStringLiteral(
        "QWidget { background: #F1E8DC; color: #2B211A; }"
        "QLabel#title { font-size: 28px; font-weight: 600; }"
        "QLabel#status { font-size: 18px; }"));

    auto *layout = new QVBoxLayout(&window);
    layout->setContentsMargins(48, 48, 48, 48);
    layout->setSpacing(24);
    layout->addStretch();

    auto *title = new QLabel(QStringLiteral("MUKEI"), &window);
    title->setObjectName(QStringLiteral("title"));
    title->setAlignment(Qt::AlignCenter);
    layout->addWidget(title);

    auto *status = new QLabel(
        QStringLiteral("Qt Widgets probe is alive.\n"
                       "No QML, Vulkan, bridge, database, or model runtime is active."),
        &window);
    status->setObjectName(QStringLiteral("status"));
    status->setAlignment(Qt::AlignCenter);
    status->setWordWrap(true);
    layout->addWidget(status);
    layout->addStretch();

    window.showFullScreen();
    qInfo("MukeiProbe stage=window_shown variant=qt-widgets-raster");
    return app.exec();
}
'''

INLINE_QML_SOURCE = r'''#include <QGuiApplication>
#include <QQmlApplicationEngine>
#include <QQmlError>
#include <QQuickWindow>
#include <QSGRendererInterface>

int main(int argc, char *argv[])
{
    qputenv("QT_LOGGING_RULES", QByteArrayLiteral("*.debug=true;qt.*=true"));
    QQuickWindow::setGraphicsApi(QSGRendererInterface::Software);
    qInfo("MukeiProbe stage=process_start variant=inline-qml-software");

    QGuiApplication app(argc, argv);
    app.setApplicationName(QStringLiteral("Mukei QML Probe"));
    qInfo("MukeiProbe stage=qguiapplication_ready variant=inline-qml-software");

    QQmlApplicationEngine engine;
    QObject::connect(&engine, &QQmlApplicationEngine::warnings, &app,
                     [](const QList<QQmlError> &warnings) {
        for (const QQmlError &warning : warnings)
            qCritical().noquote() << "MukeiProbe qml_warning" << warning.toString();
    });
    QObject::connect(&engine, &QQmlApplicationEngine::objectCreated, &app,
                     [](QObject *object, const QUrl &url) {
        qInfo().noquote() << "MukeiProbe stage=object_created ok=" << (object != nullptr)
                          << "url=" << url.toString();
    });
    QObject::connect(&engine, &QQmlApplicationEngine::objectCreationFailed, &app,
                     [] {
        qCritical("MukeiProbe stage=object_creation_failed variant=inline-qml-software");
    });

    static const char qml[] = R"QML(
import QtQuick
import QtQuick.Window

Window {
    visible: true
    width: 720
    height: 1280
    color: "#F1E8DC"
    title: "Mukei QML Probe"

    Column {
        anchors.centerIn: parent
        width: Math.min(parent.width - 64, 620)
        spacing: 24

        Text {
            width: parent.width
            horizontalAlignment: Text.AlignHCenter
            text: "MUKEI"
            color: "#2B211A"
            font.pixelSize: 46
            font.bold: true
        }
        Text {
            width: parent.width
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            text: "Inline minimal QML is alive.\nSoftware renderer active.\nNo packaged MainWindow, bridge, database, or model runtime."
            color: "#2B211A"
            font.pixelSize: 24
        }
        Rectangle {
            width: parent.width
            height: 8
            radius: 4
            color: "#B87333"
        }
    }

    Component.onCompleted: console.log("MukeiProbe stage=qml_completed variant=inline-qml-software")
}
)QML";

    engine.loadData(QByteArray(qml), QUrl(QStringLiteral("qrc:/MukeiInlineProbe.qml")));
    qInfo("MukeiProbe stage=load_data_returned variant=inline-qml-software roots=%lld",
          static_cast<long long>(engine.rootObjects().size()));
    return app.exec();
}
'''

QML_DIAGNOSTICS_SOURCE = r'''#include <QApplication>
#include <QLabel>
#include <QPlainTextEdit>
#include <QTimer>
#include <QVBoxLayout>
#include <QWidget>
#include <QQmlComponent>
#include <QQmlEngine>
#include <QQmlError>
#include <QQuickWindow>
#include <QSGRendererInterface>
#include <memory>

static QString formatErrors(const QList<QQmlError> &errors)
{
    QStringList lines;
    for (const QQmlError &error : errors)
        lines.append(error.toString());
    return lines.join(QStringLiteral("\n"));
}

int main(int argc, char *argv[])
{
    qputenv("QT_LOGGING_RULES", QByteArrayLiteral("*.debug=true;qt.*=true"));
    QQuickWindow::setGraphicsApi(QSGRendererInterface::OpenGL);
    qInfo("MukeiProbe stage=process_start variant=qml-diagnostics-widget");

    QApplication app(argc, argv);
    app.setApplicationName(QStringLiteral("Mukei QML Diagnostics"));

    QWidget window;
    window.setWindowTitle(QStringLiteral("Mukei QML Diagnostics"));
    window.setStyleSheet(QStringLiteral(
        "QWidget { background: #F1E8DC; color: #2B211A; }"
        "QLabel#title { font-size: 27px; font-weight: 600; }"
        "QPlainTextEdit { background: #FBF7F1; color: #2B211A; border: 1px solid #B87333;"
        " border-radius: 10px; padding: 12px; font-size: 15px; }"));

    auto *layout = new QVBoxLayout(&window);
    layout->setContentsMargins(28, 36, 28, 28);
    layout->setSpacing(18);

    auto *title = new QLabel(QStringLiteral("MUKEI QML DIAGNOSTICS"), &window);
    title->setObjectName(QStringLiteral("title"));
    title->setAlignment(Qt::AlignCenter);
    layout->addWidget(title);

    auto *status = new QPlainTextEdit(&window);
    status->setReadOnly(true);
    status->setPlainText(QStringLiteral(
        "PASS  Android + Qt Widgets first frame\n"
        "WAIT  Running QML component checks…"));
    layout->addWidget(status, 1);

    window.showFullScreen();
    qInfo("MukeiProbe stage=widget_window_shown variant=qml-diagnostics-widget");

    QTimer::singleShot(350, &window, [status] {
        QStringList report;
        report << QStringLiteral("PASS  Android + Qt Widgets first frame");

        auto runComponentCheck = [&report](const QString &name,
                                           const QByteArray &source,
                                           const std::function<bool(QObject *)> &verify) {
            QQmlEngine engine;
            QQmlComponent component(&engine);
            component.setData(source, QUrl(QStringLiteral("qrc:/") + name + QStringLiteral(".qml")));
            if (component.isError()) {
                report << QStringLiteral("FAIL  %1").arg(name);
                report << formatErrors(component.errors());
                return;
            }

            std::unique_ptr<QObject> object(component.create());
            if (!object) {
                report << QStringLiteral("FAIL  %1: component.create() returned null").arg(name);
                const QString details = formatErrors(component.errors());
                if (!details.isEmpty())
                    report << details;
                return;
            }

            if (!verify(object.get())) {
                report << QStringLiteral("FAIL  %1: object verification failed").arg(name);
                return;
            }

            report << QStringLiteral("PASS  %1").arg(name);
        };

        runComponentCheck(
            QStringLiteral("QtQml import + QtObject"),
            QByteArrayLiteral("import QtQml\nQtObject { property int answer: 42 }"),
            [](QObject *object) { return object->property("answer").toInt() == 42; });
        status->setPlainText(report.join(QStringLiteral("\n")));
        QApplication::processEvents();

        runComponentCheck(
            QStringLiteral("QtQuick import + Item"),
            QByteArrayLiteral("import QtQuick\nItem { width: 11; height: 13 }"),
            [](QObject *object) {
                return object->property("width").toInt() == 11
                    && object->property("height").toInt() == 13;
            });
        status->setPlainText(report.join(QStringLiteral("\n")));
        QApplication::processEvents();

        runComponentCheck(
            QStringLiteral("QtQuick.Window hidden Window"),
            QByteArrayLiteral(
                "import QtQuick\n"
                "import QtQuick.Window\n"
                "Window { visible: false; width: 17; height: 19 }"),
            [](QObject *object) {
                return object->property("width").toInt() == 17
                    && object->property("height").toInt() == 19;
            });
        status->setPlainText(report.join(QStringLiteral("\n")));
        QApplication::processEvents();

        {
            std::unique_ptr<QQuickWindow> quickWindow(new QQuickWindow);
            quickWindow->resize(20, 20);
            quickWindow->setColor(QColor(QStringLiteral("#F1E8DC")));
            report << QStringLiteral("PASS  C++ QQuickWindow construction");
        }

        report << QStringLiteral("");
        report << QStringLiteral("DONE  Send a screenshot of this report.");
        status->setPlainText(report.join(QStringLiteral("\n")));
        status->moveCursor(QTextCursor::Start);
        qInfo().noquote() << "MukeiProbe diagnostics_report\n" + report.join(QStringLiteral("\n"));
    });

    return app.exec();
}
'''

QUICK_CPP_OPENGL_SOURCE = r'''#include <QGuiApplication>
#include <QPainter>
#include <QQuickPaintedItem>
#include <QQuickWindow>
#include <QSGRendererInterface>

class ProbeItem final : public QQuickPaintedItem
{
public:
    using QQuickPaintedItem::QQuickPaintedItem;

    void paint(QPainter *painter) override
    {
        painter->setRenderHint(QPainter::Antialiasing, true);
        painter->fillRect(boundingRect(), QColor(QStringLiteral("#F1E8DC")));

        QFont titleFont;
        titleFont.setPixelSize(46);
        titleFont.setBold(true);
        painter->setFont(titleFont);
        painter->setPen(QColor(QStringLiteral("#2B211A")));
        painter->drawText(boundingRect().adjusted(40, 0, -40, -120),
                          Qt::AlignCenter,
                          QStringLiteral("MUKEI"));

        QFont bodyFont;
        bodyFont.setPixelSize(24);
        painter->setFont(bodyFont);
        painter->drawText(boundingRect().adjusted(42, 150, -42, -30),
                          Qt::AlignHCenter | Qt::AlignTop | Qt::TextWordWrap,
                          QStringLiteral(
                              "C++ Qt Quick OpenGL probe is alive.\n\n"
                              "No QML, bridge, database, or model runtime is active."));
    }
};

int main(int argc, char *argv[])
{
    qputenv("QT_LOGGING_RULES", QByteArrayLiteral("*.debug=true;qt.*=true"));
    QQuickWindow::setGraphicsApi(QSGRendererInterface::OpenGL);
    qInfo("MukeiProbe stage=process_start variant=quick-cpp-opengl");

    QGuiApplication app(argc, argv);
    app.setApplicationName(QStringLiteral("Mukei Quick OpenGL Probe"));

    QQuickWindow window;
    window.setTitle(QStringLiteral("Mukei Quick OpenGL Probe"));
    window.setColor(QColor(QStringLiteral("#F1E8DC")));
    window.resize(720, 1280);

    auto *item = new ProbeItem(window.contentItem());
    item->setWidth(window.width());
    item->setHeight(window.height());
    QObject::connect(&window, &QQuickWindow::widthChanged, item,
                     [item](int width) { item->setWidth(width); });
    QObject::connect(&window, &QQuickWindow::heightChanged, item,
                     [item](int height) { item->setHeight(height); });

    window.show();
    qInfo("MukeiProbe stage=quick_window_shown variant=quick-cpp-opengl");
    return app.exec();
}
'''

ANDROID_PROPERTIES = r'''
if(ANDROID)
    set_property(TARGET mukei PROPERTY QT_ANDROID_MIN_SDK_VERSION 29)
    set_property(TARGET mukei PROPERTY QT_ANDROID_TARGET_SDK_VERSION 35)
    if(EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/android/AndroidManifest.xml)
        set_property(TARGET mukei PROPERTY QT_ANDROID_PACKAGE_SOURCE_DIR
            ${CMAKE_CURRENT_SOURCE_DIR}/android
        )
    endif()
endif()
'''

SOURCES = {
    "qt-widgets-raster": WIDGETS_SOURCE,
    "inline-qml-software": INLINE_QML_SOURCE,
    "qml-diagnostics-widget": QML_DIAGNOSTICS_SOURCE,
    "quick-cpp-opengl": QUICK_CPP_OPENGL_SOURCE,
}

LABELS = {
    "qt-widgets-raster": "Mukei Qt Probe",
    "inline-qml-software": "Mukei QML Probe",
    "qml-diagnostics-widget": "Mukei QML Diagnostics",
    "quick-cpp-opengl": "Mukei Quick OpenGL Probe",
}


def prepare_cmake() -> None:
    text = CMAKE.read_text(encoding="utf-8")
    old = "add_executable(mukei\n"
    if old not in text:
        raise SystemExit("minimal probe preparation failed: main executable declaration not found")
    text = text.replace(old, "qt_add_executable(mukei\n", 1)

    marker = "# QuickTest executes QML directly from the filesystem."
    if marker not in text:
        raise SystemExit("minimal probe preparation failed: test-section marker not found")

    application_only = text.split(marker, 1)[0].rstrip()
    CMAKE.write_text(application_only + "\n\n" + ANDROID_PROPERTIES.lstrip(), encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("variant", choices=tuple(SOURCES))
    args = parser.parse_args()

    MAIN.write_text(SOURCES[args.variant], encoding="utf-8")
    prepare_cmake()

    manifest = MANIFEST.read_text(encoding="utf-8")
    manifest = manifest.replace('android:label="Mukei"', f'android:label="{LABELS[args.variant]}"')
    MANIFEST.write_text(manifest, encoding="utf-8")

    print(f"Prepared minimal Android probe: {args.variant}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
