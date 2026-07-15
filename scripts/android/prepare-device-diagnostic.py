#!/usr/bin/env python3
"""Prepare deterministic physical-device diagnostic variants of the Android app.

The transform is intentionally strict: every expected source fragment must match
exactly once, otherwise the build stops rather than silently producing an
unknown diagnostic APK.
"""
from __future__ import annotations

import argparse
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MAIN = ROOT / "qml/main.cpp"


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"diagnostic preparation failed: {label} matched {count} times")
    return text.replace(old, new, 1)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", choices=("opengl", "safe-start"), required=True)
    args = parser.parse_args()

    text = MAIN.read_text(encoding="utf-8")

    text = replace_once(
        text,
        "#include <QQmlApplicationEngine>\n",
        "#include <QQmlApplicationEngine>\n#include <QQmlError>\n",
        "QQmlError include insertion",
    )

    old_auto_initialize = '''    bool autoInitialize() const
    {
#ifdef MUKEI_USE_REAL_BRIDGE
        // Secure database bootstrap is owned by the native application runtime.
        // QML never receives or injects plaintext database key material.
        return true;
#else
        return true;
#endif
    }
'''
    if args.mode == "safe-start":
        new_auto_initialize = '''    bool autoInitialize() const
    {
        // Physical-device diagnostic mode: render the complete QML shell but
        // do not enter Keystore, SQLCipher, model, or inference bootstrap.
        return false;
    }
'''
    else:
        new_auto_initialize = '''    bool autoInitialize() const
    {
        // Normal physical-device diagnostic mode: retain production bootstrap.
        return true;
    }
'''
    text = replace_once(
        text,
        old_auto_initialize,
        new_auto_initialize,
        "autoInitialize diagnostic mode",
    )

    old_graphics = '''#ifdef Q_OS_ANDROID
    if (QOperatingSystemVersion::current() >= QOperatingSystemVersion(QOperatingSystemVersion::Android, 31)) {
        QQuickWindow::setGraphicsApi(QSGRendererInterface::Vulkan);
    } else {
        QQuickWindow::setGraphicsApi(QSGRendererInterface::OpenGL);
    }
#endif
'''
    new_graphics = f'''#ifdef Q_OS_ANDROID
    // Device remediation: Vulkan must never be selected solely from Android API
    // level. Force the broadly supported OpenGL RHI while Samsung/Exynos device
    // compatibility is being established.
    QQuickWindow::setGraphicsApi(QSGRendererInterface::OpenGL);
    qInfo().noquote() << "MukeiStartup stage=graphics_api backend=OpenGL mode={args.mode}";
#endif
'''
    text = replace_once(text, old_graphics, new_graphics, "Android graphics selection")

    text = replace_once(
        text,
        "    QGuiApplication app(argc, argv);\n",
        f'''    QGuiApplication app(argc, argv);
    qInfo().noquote() << "MukeiStartup stage=process_start"
                      << "mode={args.mode}"
                      << "qt=" << qVersion()
                      << "platform=" << QGuiApplication::platformName();
''',
        "process startup logging",
    )

    text = replace_once(
        text,
        "    MukeiClipboard clipboard;\n",
        '''    qInfo().noquote() << "MukeiStartup stage=runtime_objects_begin";
    MukeiClipboard clipboard;
''',
        "runtime object begin logging",
    )

    bridge_block_end = '''#else
    MukeiAgentStub agent;
    MukeiBridgeStub bridge;
    SafRegistryStub safRegistry;
#endif

    QQmlApplicationEngine engine;
'''
    bridge_block_replacement = '''#else
    MukeiAgentStub agent;
    MukeiBridgeStub bridge;
    SafRegistryStub safRegistry;
#endif
    qInfo().noquote() << "MukeiStartup stage=runtime_objects_ready";

    QQmlApplicationEngine engine;
    qInfo().noquote() << "MukeiStartup stage=qml_engine_created";
'''
    text = replace_once(
        text,
        bridge_block_end,
        bridge_block_replacement,
        "runtime object ready logging",
    )

    old_engine_tail = '''    QObject::connect(&engine, &QQmlApplicationEngine::objectCreationFailed, &app, [] {
        QCoreApplication::exit(-1);
    }, Qt::QueuedConnection);

    QTimer::singleShot(100, &engine, [&engine] {
        engine.load(QUrl(QStringLiteral("qrc:/qt/qml/com/mukei/app/MainWindow.qml")));
    });
'''
    new_engine_tail = '''    QObject::connect(&engine, &QQmlApplicationEngine::warnings, &app,
                     [](const QList<QQmlError> &warnings) {
        for (const QQmlError &warning : warnings) {
            qCritical().noquote() << "MukeiQmlWarning"
                                  << warning.url().toString()
                                  << "line=" << warning.line()
                                  << "column=" << warning.column()
                                  << warning.description();
        }
    });
    QObject::connect(&engine, &QQmlApplicationEngine::objectCreated, &app,
                     [](QObject *object, const QUrl &url) {
        qInfo().noquote() << "MukeiStartup stage=root_object_result"
                          << "created=" << (object != nullptr)
                          << "url=" << url.toString();
    });
    QObject::connect(&engine, &QQmlApplicationEngine::objectCreationFailed, &app,
                     [](const QUrl &url) {
        qCritical().noquote() << "MukeiStartup stage=root_object_failed"
                              << "url=" << url.toString();
        QCoreApplication::exit(-1);
    }, Qt::QueuedConnection);

    QTimer::singleShot(100, &engine, [&engine] {
        const QUrl rootUrl(QStringLiteral("qrc:/qt/qml/com/mukei/app/MainWindow.qml"));
        qInfo().noquote() << "MukeiStartup stage=qml_load_begin"
                          << "url=" << rootUrl.toString();
        engine.load(rootUrl);
        qInfo().noquote() << "MukeiStartup stage=qml_load_return"
                          << "root_count=" << engine.rootObjects().size();
    });
'''
    text = replace_once(text, old_engine_tail, new_engine_tail, "QML diagnostic logging")

    MAIN.write_text(text, encoding="utf-8")
    print(f"Prepared Android device diagnostic variant: {args.mode}")
    print("  graphics backend: OpenGL")
    print(f"  automatic native bootstrap: {'disabled' if args.mode == 'safe-start' else 'enabled'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
