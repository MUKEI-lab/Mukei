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
MANIFEST = ROOT / "qml/android/AndroidManifest.xml"

WIDGETS_SOURCE = r'''#include <QApplication>
#include <QLabel>
#include <QLoggingCategory>
#include <QScreen>
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
#include <QTimer>

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
    id: root
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


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("variant", choices=("qt-widgets-raster", "inline-qml-software"))
    args = parser.parse_args()

    source = WIDGETS_SOURCE if args.variant == "qt-widgets-raster" else INLINE_QML_SOURCE
    MAIN.write_text(source, encoding="utf-8")

    manifest = MANIFEST.read_text(encoding="utf-8")
    label = "Mukei Qt Probe" if args.variant == "qt-widgets-raster" else "Mukei QML Probe"
    manifest = manifest.replace('android:label="Mukei"', f'android:label="{label}"')
    MANIFEST.write_text(manifest, encoding="utf-8")

    print(f"Prepared minimal Android probe: {args.variant}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
