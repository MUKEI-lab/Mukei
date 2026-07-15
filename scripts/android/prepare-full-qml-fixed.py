#!/usr/bin/env python3
"""Prepare the actual Mukei QML UI with the corrected Android entrypoint.

This keeps the product QML tree intact while disabling the real native bridge in
its build workflow. The purpose is to prove the production UI can load after
fixing the resource/module entrypoint, without database or model-runtime noise.
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

    wrong_url = 'engine.load(QUrl(QStringLiteral("qrc:/qt/qml/com/mukei/app/MainWindow.qml")));'
    module_load = 'engine.loadFromModule(QStringLiteral("com.mukei.app"), QStringLiteral("MainWindow"));'
    if wrong_url not in text:
        raise SystemExit("expected incorrect MainWindow QRC URL was not found")
    text = text.replace(wrong_url, module_load, 1)

    # The Samsung M34 probe proved the OpenGL Qt Quick path works. Keep this
    # device-validation build deterministic and avoid the unconditional Vulkan
    # branch while the production renderer policy is being redesigned.
    if "QSGRendererInterface::Vulkan" not in text:
        raise SystemExit("expected Android Vulkan selection was not found")
    text = text.replace("QSGRendererInterface::Vulkan", "QSGRendererInterface::OpenGL", 1)

    failure_block = '''    QObject::connect(&engine, &QQmlApplicationEngine::objectCreationFailed, &app, [] {
        QCoreApplication::exit(-1);
    }, Qt::QueuedConnection);
'''
    diagnostics_block = '''    QObject::connect(&engine, &QQmlApplicationEngine::warnings, &app,
                     [](const QList<QQmlError> &warnings) {
        for (const QQmlError &warning : warnings)
            qCritical().noquote() << "MukeiStartup qml_warning" << warning.toString();
    });
    QObject::connect(&engine, &QQmlApplicationEngine::objectCreated, &app,
                     [](QObject *object, const QUrl &url) {
        qInfo().noquote() << "MukeiStartup root_object_created ok=" << (object != nullptr)
                          << "url=" << url.toString();
    });
    QObject::connect(&engine, &QQmlApplicationEngine::objectCreationFailed, &app, [] {
        qCritical("MukeiStartup root_object_failed");
        QCoreApplication::exit(-1);
    }, Qt::QueuedConnection);
'''
    if failure_block not in text:
        raise SystemExit("expected objectCreationFailed block was not found")
    text = text.replace(failure_block, diagnostics_block, 1)

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
    text = text.replace('android:label="Mukei"', 'android:label="Mukei Full UI Fix"')
    MANIFEST.write_text(text, encoding="utf-8")


def main() -> int:
    patch_main()
    patch_cmake()
    patch_manifest()
    print("Prepared full Mukei QML UI with corrected module entrypoint and OpenGL")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
