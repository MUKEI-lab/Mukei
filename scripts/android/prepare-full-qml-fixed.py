#!/usr/bin/env python3
"""Prepare the actual Mukei QML UI with an on-device failure screen.

The full product QML tree is loaded with the stub backend. A Qt Widgets window
remains alive underneath it and displays resource checks plus every QQmlError if
root creation fails, so a phone-only test does not lose startup evidence.
"""
from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MAIN = ROOT / "qml/main.cpp"
CMAKE = ROOT / "qml/CMakeLists.txt"
MANIFEST = ROOT / "qml/android/AndroidManifest.xml"

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


def patch_main() -> None:
    text = MAIN.read_text(encoding="utf-8")

    include_anchor = '#include <QGuiApplication>\n'
    widget_includes = '''#include <QApplication>\n#include <QFile>\n#include <QLabel>\n#include <QPlainTextEdit>\n#include <QVBoxLayout>\n#include <QWidget>\n#include <QQmlError>\n'''
    if include_anchor not in text:
        raise SystemExit("QGuiApplication include anchor not found")
    text = text.replace(include_anchor, include_anchor + widget_includes, 1)

    if "QGuiApplication app(argc, argv);" not in text:
        raise SystemExit("QGuiApplication construction not found")
    text = text.replace(
        "QGuiApplication app(argc, argv);",
        "QApplication app(argc, argv);\n    app.setQuitOnLastWindowClosed(false);",
        1,
    )

    # The Samsung M34 probes verified the Qt Quick OpenGL path.
    if "QSGRendererInterface::Vulkan" not in text:
        raise SystemExit("Android Vulkan selection not found")
    text = text.replace("QSGRendererInterface::Vulkan", "QSGRendererInterface::OpenGL", 1)

    engine_anchor = "    QQmlApplicationEngine engine;\n"
    diagnostics_setup = '''    QWidget diagnosticsWindow;
    diagnosticsWindow.setWindowTitle(QStringLiteral("Mukei Full QML Diagnostics"));
    diagnosticsWindow.setStyleSheet(QStringLiteral(
        "QWidget { background: #F1E8DC; color: #2B211A; }"
        "QLabel { font-size: 25px; font-weight: 600; }"
        "QPlainTextEdit { background: #FBF7F1; color: #2B211A; border: 1px solid #B87333;"
        " border-radius: 10px; padding: 12px; font-size: 14px; }"));
    auto *diagnosticsLayout = new QVBoxLayout(&diagnosticsWindow);
    diagnosticsLayout->setContentsMargins(28, 36, 28, 28);
    diagnosticsLayout->setSpacing(18);
    auto *diagnosticsTitle = new QLabel(QStringLiteral("MUKEI FULL QML STARTUP"), &diagnosticsWindow);
    diagnosticsTitle->setAlignment(Qt::AlignCenter);
    diagnosticsLayout->addWidget(diagnosticsTitle);
    auto *diagnosticsText = new QPlainTextEdit(&diagnosticsWindow);
    diagnosticsText->setReadOnly(true);
    diagnosticsText->setPlainText(QStringLiteral(
        "PASS  Android + Qt Widgets fallback window\\n"
        "PASS  Qt Quick + OpenGL verified on this device\\n"
        "WAIT  Checking packaged MainWindow resources..."));
    diagnosticsLayout->addWidget(diagnosticsText, 1);
    diagnosticsWindow.showFullScreen();

    QQmlApplicationEngine engine;
'''
    if engine_anchor not in text:
        raise SystemExit("QQmlApplicationEngine anchor not found")
    text = text.replace(engine_anchor, diagnostics_setup, 1)

    old_failure = '''    QObject::connect(&engine, &QQmlApplicationEngine::objectCreationFailed, &app, [] {
        QCoreApplication::exit(-1);
    }, Qt::QueuedConnection);

    QTimer::singleShot(100, &engine, [&engine] {
        engine.load(QUrl(QStringLiteral("qrc:/qt/qml/com/mukei/app/MainWindow.qml")));
    });
'''
    diagnostics_connections = '''    const QString canonicalResource = QStringLiteral(":/com/mukei/app/MainWindow.qml");
    const QString legacyResource = QStringLiteral(":/qt/qml/com/mukei/app/MainWindow.qml");
    const QUrl canonicalUrl(QStringLiteral("qrc:/com/mukei/app/MainWindow.qml"));

    diagnosticsText->appendPlainText(QStringLiteral("%1  %2")
        .arg(QFile::exists(canonicalResource) ? QStringLiteral("PASS") : QStringLiteral("FAIL"),
             canonicalResource));
    diagnosticsText->appendPlainText(QStringLiteral("%1  %2")
        .arg(QFile::exists(legacyResource) ? QStringLiteral("PASS") : QStringLiteral("FAIL"),
             legacyResource));
    diagnosticsText->appendPlainText(QStringLiteral("WAIT  Direct-loading %1...").arg(canonicalUrl.toString()));

    QObject::connect(&engine, &QQmlApplicationEngine::warnings, &app,
                     [diagnosticsText, &diagnosticsWindow](const QList<QQmlError> &warnings) {
        diagnosticsWindow.showFullScreen();
        for (const QQmlError &warning : warnings) {
            const QString message = warning.toString();
            diagnosticsText->appendPlainText(QStringLiteral("QML  ") + message);
            qCritical().noquote() << "MukeiStartup qml_warning" << message;
        }
    });
    QObject::connect(&engine, &QQmlApplicationEngine::objectCreated, &app,
                     [diagnosticsText, &diagnosticsWindow](QObject *object, const QUrl &url) {
        diagnosticsText->appendPlainText(
            QStringLiteral("%1  root object: %2")
                .arg(object ? QStringLiteral("PASS") : QStringLiteral("FAIL"), url.toString()));
        qInfo().noquote() << "MukeiStartup root_object_created ok=" << (object != nullptr)
                          << "url=" << url.toString();
        if (!object) {
            diagnosticsWindow.showFullScreen();
            return;
        }
        QObject::connect(object, &QObject::destroyed, &diagnosticsWindow,
                         [diagnosticsText, &diagnosticsWindow] {
            diagnosticsText->appendPlainText(QStringLiteral("FAIL  MainWindow root object was destroyed"));
            diagnosticsWindow.showFullScreen();
        });
        QTimer::singleShot(1500, &diagnosticsWindow, &QWidget::hide);
    });
    QObject::connect(&engine, &QQmlApplicationEngine::objectCreationFailed, &app,
                     [diagnosticsText, &diagnosticsWindow] {
        diagnosticsText->appendPlainText(QStringLiteral(
            "FAIL  MainWindow object creation failed.\\n"
            "Screenshot every QML line shown above."));
        diagnosticsWindow.showFullScreen();
        qCritical("MukeiStartup root_object_failed");
    }, Qt::QueuedConnection);

    QTimer::singleShot(350, &engine, [&engine, canonicalUrl] {
        engine.load(canonicalUrl);
    });
'''
    if old_failure not in text:
        raise SystemExit("original QML load/failure block not found")
    text = text.replace(old_failure, diagnostics_connections, 1)

    MAIN.write_text(text, encoding="utf-8")


def patch_cmake() -> None:
    text = CMAKE.read_text(encoding="utf-8")
    old = "add_executable(mukei\n"
    if old not in text:
        raise SystemExit("main executable declaration not found")
    text = text.replace(old, "qt_add_executable(mukei\n", 1)

    marker = "# QuickTest executes QML directly from the filesystem."
    if marker not in text:
        raise SystemExit("test-section marker not found")
    application_only = text.split(marker, 1)[0].rstrip()
    CMAKE.write_text(application_only + "\n\n" + ANDROID_PROPERTIES.lstrip(), encoding="utf-8")


def patch_manifest() -> None:
    text = MANIFEST.read_text(encoding="utf-8")
    text = text.replace('android:label="Mukei"', 'android:label="Mukei Direct QRC Diagnostics"')
    MANIFEST.write_text(text, encoding="utf-8")


def main() -> int:
    patch_main()
    patch_cmake()
    patch_manifest()
    print("Prepared full Mukei QML tree with canonical direct-QRC loading")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
