cmake_minimum_required(VERSION 3.22)

# Product semantic version has one source of truth: rust/Cargo.toml [workspace.package].
# Protocol and database schema versions intentionally remain independent.
file(READ "${CMAKE_CURRENT_SOURCE_DIR}/../rust/Cargo.toml" _MUKEI_WORKSPACE_MANIFEST)
string(REGEX MATCH "\nversion[ \t]*=[ \t]*\"([0-9]+[.][0-9]+[.][0-9]+)\"" _MUKEI_VERSION_MATCH "${_MUKEI_WORKSPACE_MANIFEST}")
if(NOT CMAKE_MATCH_1)
    message(FATAL_ERROR "Could not derive Mukei product version from rust/Cargo.toml")
endif()
set(MUKEI_PRODUCT_VERSION "${CMAKE_MATCH_1}")
project(MukeiQml VERSION ${MUKEI_PRODUCT_VERSION} LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)
set(QT_QUICK_CONTROLS_STYLE "Basic")
include(CTest)

if(ANDROID)
    set(_MUKEI_REAL_BRIDGE_DEFAULT ON)
else()
    set(_MUKEI_REAL_BRIDGE_DEFAULT OFF)
endif()
option(MUKEI_USE_REAL_BRIDGE "Instantiate CXX-Qt bridge objects instead of local QML stubs" ${_MUKEI_REAL_BRIDGE_DEFAULT})

# Qt Quick Effects / MultiEffect entered the supported Qt 6 API in 6.5.
# Keep the declared build floor aligned with the QML imports instead of
# allowing configure to succeed against an SDK that cannot load the module.
find_package(Qt6 6.5 REQUIRED COMPONENTS
    Core Gui Quick Qml QuickControls2 QuickDialogs2 QuickLayouts QuickTest Test Widgets Svg
)

# Real bridge integration follows the native platform contract:
# Android consumes the prebuilt `cdylib` .so, while desktop CMake uses
# CXX-Qt's official Corrosion wrapper. The official import propagates
# generated headers, native support libraries, Qt link metadata, and
# initializer objects that cannot be recovered by linking Cargo's raw
# static archive directly.
if(ANDROID)
    set(_MUKEI_BRIDGE_DEFAULT
        ${CMAKE_CURRENT_SOURCE_DIR}/../rust/target/aarch64-linux-android/release/libmukei_bridge.so
    )
    set(MUKEI_BRIDGE_LIB "${_MUKEI_BRIDGE_DEFAULT}"
        CACHE FILEPATH "Path to the Cargo-built Android mukei-bridge shared library"
    )
elseif(MUKEI_USE_REAL_BRIDGE)
    include(FetchContent)
    FetchContent_Declare(
        CxxQt
        GIT_REPOSITORY https://github.com/KDAB/cxx-qt-cmake.git
        GIT_TAG fcc1dbfb97712f1253b52a6f565298dc7fd0a6e3
    )
    FetchContent_MakeAvailable(CxxQt)

    cxx_qt_import_crate(
        MANIFEST_PATH ${CMAKE_CURRENT_SOURCE_DIR}/../rust/Cargo.toml
        CRATES mukei-bridge
        LOCKED
        QT_MODULES Qt6::Core
    )
endif()

add_executable(mukei
    main.cpp
    timeline_model.cpp
    timeline_model.h
    qml.qrc
)
target_compile_definitions(mukei PRIVATE MUKEI_PRODUCT_VERSION="${MUKEI_PRODUCT_VERSION}")

set(_MUKEI_QML_SINGLETONS
    events/EventDispatcher.qml
    architecture/AppCoordinator.qml
    architecture/AppStateHub.qml
    architecture/IntentDispatcher.qml
    architecture/PresentationPolicy.qml
    architecture/SnapshotController.qml
    stores/ContractStore.qml
    stores/LifecycleStore.qml
    stores/CapabilityStore.qml
    stores/ConversationStore.qml
    stores/NavigationStore.qml
    stores/UiSessionStore.qml
    stores/ChatStore.qml
    stores/RecoveryStore.qml
    stores/ModelStore.qml
    stores/DownloadStore.qml
    stores/DocumentStore.qml
    stores/StorageStore.qml
    stores/SettingsStore.qml
    stores/ResponsiveStore.qml
    stores/OperationStore.qml
    stores/ErrorStore.qml
    stores/DiagnosticsStore.qml
    stores/AccessibilityStore.qml
    theme/Theme.qml
    theme/Type.qml
    theme/Spacing.qml
    theme/Motion.qml
)
set_source_files_properties(${_MUKEI_QML_SINGLETONS} PROPERTIES QT_QML_SINGLETON_TYPE TRUE)

qt_add_qml_module(mukei
    URI com.mukei.app
    VERSION 1.0
    RESOURCE_PREFIX /
    QML_FILES
        MainWindow.qml
        events/EventDispatcher.qml
        architecture/AppCoordinator.qml
        architecture/AppStateHub.qml
        architecture/IntentDispatcher.qml
        architecture/PresentationPolicy.qml
        architecture/SnapshotController.qml
        stores/ContractStore.qml
        stores/LifecycleStore.qml
        stores/CapabilityStore.qml
        stores/ConversationStore.qml
        stores/NavigationStore.qml
        stores/UiSessionStore.qml
        stores/ChatStore.qml
        stores/RecoveryStore.qml
        stores/ModelStore.qml
        stores/DownloadStore.qml
        stores/DocumentStore.qml
        stores/StorageStore.qml
        stores/SettingsStore.qml
        stores/ResponsiveStore.qml
        stores/OperationStore.qml
        stores/ErrorStore.qml
        stores/DiagnosticsStore.qml
        stores/AccessibilityStore.qml
        shell/AppShell.qml
        shell/AdaptiveNavigation.qml
        shell/RouterHost.qml
        shell/BannerHost.qml
        shell/SnackbarHost.qml
        shell/OperationOverlayHost.qml
        shell/SheetHost.qml
        shell/DialogHost.qml
        theme/Theme.qml
        theme/Type.qml
        theme/Spacing.qml
        theme/Motion.qml
        components/AccessibilityAnnouncer.qml
        components/AIMessageBubble.qml
        components/BranchSwitcherSheet.qml
        components/ChatComposer.qml
        components/ChatTimelineEvent.qml
        components/CodeBlockComponent.qml
        components/ConfirmationDialog.qml
        components/ConversationList.qml
        components/CopyButton.qml
        components/DestructiveButton.qml
        components/DestructiveConfirmDialog.qml
        components/FontLoader.qml
        components/FullScreenModal.qml
        components/GhostButton.qml
        components/HapticFeedback.qml
        components/IconButton.qml
        components/LeftDrawer.qml
        components/MarkdownRenderer.qml
        components/MukeiIcon.qml
        components/ModalSheet.qml
        components/NetworkBanner.qml
        components/PrimaryButton.qml
        components/ProgressBar.qml
        components/PromptCard.qml
        components/RAGChunkCard.qml
        components/SearchField.qml
        components/SecondaryButton.qml
        components/SettingsTextField.qml
        components/SkeletonLoader.qml
        components/Spinner.qml
        components/StatusPill.qml
        components/StoragePressureCard.qml
        components/StreamingCaret.qml
        components/ThinkingAccordion.qml
        components/ToastNotification.qml
        components/ToolResultCard.qml
        components/UserMessageBubble.qml
        screens/ChatScreen.qml
        screens/CompatibilityScreen.qml
        screens/DownloadsScreen.qml
        screens/DocumentsScreen.qml
        screens/DiagnosticsScreen.qml
        screens/EmptyChatScreen.qml
        screens/ModelManagerScreen.qml
        screens/ModelPickerScreen.qml
        screens/RagRebuildPromptScreen.qml
        screens/RecoveryScreen.qml
        screens/SafeModeScreen.qml
        screens/SettingsScreen.qml
        screens/StartupScreen.qml
        screens/ToolResultExpandedScreen.qml
        screens/VerificationScreen.qml
        screens/WelcomeScreen.qml
    RESOURCES
        events/qmldir
        architecture/qmldir
        stores/qmldir
        shell/qmldir
        theme/qmldir
        components/qmldir
        screens/qmldir
)

target_link_libraries(mukei PRIVATE
    Qt6::Core
    Qt6::Gui
    Qt6::Quick
    Qt6::Qml
    Qt6::QuickControls2
    Qt6::QuickDialogs2
    Qt6::QuickLayouts
    Qt6::Widgets
    Qt6::Svg
)

if(MUKEI_USE_REAL_BRIDGE)
    target_compile_definitions(mukei PRIVATE MUKEI_USE_REAL_BRIDGE=1)

    if(ANDROID)
        if(EXISTS ${MUKEI_BRIDGE_LIB})
            target_link_libraries(mukei PRIVATE ${MUKEI_BRIDGE_LIB})
        else()
            message(FATAL_ERROR
                "MUKEI_USE_REAL_BRIDGE=ON but the Android bridge library does not exist: ${MUKEI_BRIDGE_LIB}")
        endif()

        # Android still consumes a Cargo-built cdylib, so its generated
        # CXX-Qt headers are discovered from the Cargo target tree.
        file(GLOB_RECURSE _MUKEI_CXXQT_GENERATED_HEADERS
            LIST_DIRECTORIES FALSE
            "${CMAKE_CURRENT_SOURCE_DIR}/../rust/target/*.h"
        )
        set(_MUKEI_CXXQT_INCLUDE_DIRS "")
        set(_MUKEI_BRIDGE_GENERATED_HEADER_FOUND FALSE)
        foreach(_MUKEI_CXXQT_HEADER IN LISTS _MUKEI_CXXQT_GENERATED_HEADERS)
            if(_MUKEI_CXXQT_HEADER MATCHES "^(.*[/]cxxqtbuild[/]include)/.+[.]h$")
                list(APPEND _MUKEI_CXXQT_INCLUDE_DIRS "${CMAKE_MATCH_1}")
            endif()
            if(_MUKEI_CXXQT_HEADER MATCHES "/mukei-bridge/src/lib[.]cxxqt[.]h$")
                set(_MUKEI_BRIDGE_GENERATED_HEADER_FOUND TRUE)
            endif()
        endforeach()
        list(REMOVE_DUPLICATES _MUKEI_CXXQT_INCLUDE_DIRS)

        if(NOT _MUKEI_BRIDGE_GENERATED_HEADER_FOUND)
            message(FATAL_ERROR
                "MUKEI_USE_REAL_BRIDGE=ON but the generated CXX-Qt header "
                "mukei-bridge/src/lib.cxxqt.h was not found under rust/target. "
                "Build mukei-bridge with Cargo before configuring QML.")
        endif()

        target_include_directories(mukei PRIVATE ${_MUKEI_CXXQT_INCLUDE_DIRS})
    else()
        if(NOT TARGET mukei_bridge)
            message(FATAL_ERROR "CXX-Qt failed to import the mukei_bridge Cargo target")
        endif()
        target_link_libraries(mukei PRIVATE mukei_bridge)
    endif()
endif()

qt_add_executable(tst_EventDispatcher
    tests/tst_EventDispatcher.cpp
)

target_compile_definitions(tst_EventDispatcher PRIVATE
    EVENT_DISPATCHER_QML_SOURCE_DIR="${CMAKE_CURRENT_SOURCE_DIR}/tests/tst_EventDispatcher.qml"
)

target_link_libraries(tst_EventDispatcher PRIVATE
    Qt6::Core
    Qt6::Gui
    Qt6::Quick
    Qt6::Qml
    Qt6::QuickLayouts
    Qt6::QuickTest
)

qt_add_executable(tst_Security
    tests/tst_Security.cpp
)

target_link_libraries(tst_Security PRIVATE
    Qt6::Core
    Qt6::Gui
    Qt6::Quick
    Qt6::Qml
    Qt6::QuickControls2
    Qt6::QuickDialogs2
    Qt6::QuickLayouts
    Qt6::QuickTest
)

qt_add_executable(tst_QmlArchitecture
    tests/tst_QmlArchitecture.cpp
)

target_compile_definitions(tst_QmlArchitecture PRIVATE
    QML_ARCHITECTURE_TEST_SOURCE_DIR="${CMAKE_CURRENT_SOURCE_DIR}/tests"
)

target_link_libraries(tst_QmlArchitecture PRIVATE
    Qt6::Core
    Qt6::Gui
    Qt6::Quick
    Qt6::Qml
    Qt6::QuickControls2
    Qt6::QuickDialogs2
    Qt6::QuickLayouts
    Qt6::QuickTest
)

qt_add_executable(tst_TimelineModel
    tests/tst_TimelineModel.cpp
    timeline_model.cpp
    timeline_model.h
)

target_link_libraries(tst_TimelineModel PRIVATE
    Qt6::Core
    Qt6::Test
)

add_test(NAME tst_QmlArchitecture COMMAND tst_QmlArchitecture -platform offscreen)
set_tests_properties(tst_QmlArchitecture PROPERTIES ENVIRONMENT "QT_QPA_PLATFORM=offscreen")

add_test(NAME tst_TimelineModel COMMAND tst_TimelineModel)

add_test(NAME tst_EventDispatcher COMMAND tst_EventDispatcher -platform offscreen -input ${CMAKE_CURRENT_SOURCE_DIR}/tests/tst_EventDispatcher.qml)
set_tests_properties(tst_EventDispatcher PROPERTIES
    ENVIRONMENT "QT_QPA_PLATFORM=offscreen"
)

add_test(NAME tst_Security COMMAND tst_Security -platform offscreen -input ${CMAKE_CURRENT_SOURCE_DIR}/tests/tst_Security.qml)
set_tests_properties(tst_Security PROPERTIES
    ENVIRONMENT "QT_QPA_PLATFORM=offscreen"
)

if(ANDROID)
    set_property(TARGET mukei PROPERTY QT_ANDROID_MIN_SDK_VERSION 29)
    set_property(TARGET mukei PROPERTY QT_ANDROID_TARGET_SDK_VERSION 35)
    # Only wire the package-source directory when it exists on disk. This
    # lets the desktop CI configure without carrying the android/ payload
    # and keeps `cmake -B build` a hard-error-free step for contributors
    # who never touch the Android build.
    if(EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/android/AndroidManifest.xml)
        set_property(TARGET mukei PROPERTY QT_ANDROID_PACKAGE_SOURCE_DIR
            ${CMAKE_CURRENT_SOURCE_DIR}/android
        )
    endif()
endif()
