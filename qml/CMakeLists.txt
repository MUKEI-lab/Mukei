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
option(MUKEI_USE_NATIVE_INFERENCE "Link and package the real Mukei llama native capsule" ${_MUKEI_REAL_BRIDGE_DEFAULT})

if(ANDROID)
    set(_MUKEI_LLAMA_ABI "${ANDROID_ABI}")
else()
    set(_MUKEI_LLAMA_ABI "host")
endif()
set(MUKEI_LLAMA_NATIVE_LIB
    "${CMAKE_CURRENT_SOURCE_DIR}/../rust/llama-cpp-prebuilt/prebuilt/${_MUKEI_LLAMA_ABI}/${CMAKE_SHARED_LIBRARY_PREFIX}mukei_llama_native${CMAKE_SHARED_LIBRARY_SUFFIX}"
    CACHE FILEPATH "Path to the Mukei llama native inference capsule")

# QtQuick.Effects is a QML module consumed by QML imports, not a public CMake
# link component. Keep the Qt 6.5 floor for MultiEffect while requiring only
# the C++ targets that this build actually links.
find_package(Qt6 6.5 REQUIRED COMPONENTS
    Core Gui Quick Qml QuickControls2 QuickDialogs2 QuickLayouts QuickTest Test Widgets Svg
)

# Cargo build directory selection. Android builds still cross-compile to
# `aarch64-linux-android/release`; every other host (Linux/macOS desktop
# smoke-runs) uses the plain `release` directory. The Qt executable links
# the Rust staticlib so CXX-Qt generated QObject implementation symbols are retained.

if(ANDROID)
    set(_MUKEI_BRIDGE_DEFAULT
        ${CMAKE_CURRENT_SOURCE_DIR}/../rust/target/aarch64-linux-android/release/libmukei_bridge.a
    )
elseif(APPLE)
    set(_MUKEI_BRIDGE_DEFAULT
        ${CMAKE_CURRENT_SOURCE_DIR}/../rust/target/release/libmukei_bridge.a
    )
else()
    set(_MUKEI_BRIDGE_DEFAULT
        ${CMAKE_CURRENT_SOURCE_DIR}/../rust/target/release/libmukei_bridge.a
    )
endif()
set(MUKEI_BRIDGE_LIB "${_MUKEI_BRIDGE_DEFAULT}"
    CACHE FILEPATH "Path to the Cargo-built mukei-bridge static library"
)

# CXX-Qt generated headers normally live under Cargo's hashed OUT_DIR, which
# is not a stable include path for an external CMake build. The Rust bridge
# build exports its generated artifacts into this deterministic directory.
set(MUKEI_CXX_QT_EXPORT_DIR
    "${CMAKE_CURRENT_SOURCE_DIR}/../rust/target/cxxqt-export"
    CACHE PATH "Directory where Cargo exports CXX-Qt bridge artifacts"
)
set(MUKEI_CXX_QT_INCLUDE_DIR
    "${MUKEI_CXX_QT_EXPORT_DIR}/crates/mukei-bridge/include"
    CACHE PATH "Include root containing generated mukei_bridge CXX-Qt headers"
)

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

if(EXISTS ${MUKEI_BRIDGE_LIB})
    target_link_libraries(mukei PRIVATE ${MUKEI_BRIDGE_LIB})
elseif(MUKEI_USE_REAL_BRIDGE)
    message(FATAL_ERROR "MUKEI_USE_REAL_BRIDGE=ON but MUKEI_BRIDGE_LIB does not exist: ${MUKEI_BRIDGE_LIB}")
endif()

if(MUKEI_USE_NATIVE_INFERENCE)
    if(NOT EXISTS "${MUKEI_LLAMA_NATIVE_LIB}")
        message(FATAL_ERROR
            "MUKEI_USE_NATIVE_INFERENCE=ON but the native capsule does not exist: ${MUKEI_LLAMA_NATIVE_LIB}")
    endif()
    add_library(mukei_llama_native_runtime SHARED IMPORTED GLOBAL)
    set_target_properties(mukei_llama_native_runtime PROPERTIES
        IMPORTED_LOCATION "${MUKEI_LLAMA_NATIVE_LIB}")
    target_link_libraries(mukei PRIVATE mukei_llama_native_runtime)
    if(ANDROID)
        set_property(TARGET mukei APPEND PROPERTY QT_ANDROID_EXTRA_LIBS "${MUKEI_LLAMA_NATIVE_LIB}")
    endif()
endif()

if(MUKEI_USE_REAL_BRIDGE)
    set(_MUKEI_CXX_QT_HEADER
        "${MUKEI_CXX_QT_INCLUDE_DIR}/mukei-bridge/src/lib.cxxqt.h"
    )
    if(NOT EXISTS "${_MUKEI_CXX_QT_HEADER}")
        message(FATAL_ERROR
            "MUKEI_USE_REAL_BRIDGE=ON but the generated CXX-Qt header does not exist: "
            "${_MUKEI_CXX_QT_HEADER}. Build mukei-bridge with CXX_QT_EXPORT_DIR set to "
            "${MUKEI_CXX_QT_EXPORT_DIR} and CXX_QT_EXPORT_CRATE_mukei-bridge enabled."
        )
    endif()
    target_include_directories(mukei PRIVATE "${MUKEI_CXX_QT_INCLUDE_DIR}")
    target_compile_definitions(mukei PRIVATE MUKEI_USE_REAL_BRIDGE=1)
endif()

# QuickTest executes QML directly from the filesystem. The production
# subdirectory qmldir files intentionally prefer the compiled qrc module, but
# those resources belong to the mukei executable and are not linked into the
# standalone test executables. Stage a test-only source tree without those
# redirects, then regenerate only the singleton declarations required by
# filesystem directory imports.
set(_MUKEI_QML_TEST_SOURCE_ROOT "${CMAKE_CURRENT_BINARY_DIR}/qml-test-source")
file(REMOVE_RECURSE "${_MUKEI_QML_TEST_SOURCE_ROOT}")
file(MAKE_DIRECTORY "${_MUKEI_QML_TEST_SOURCE_ROOT}")
file(COPY "${CMAKE_CURRENT_SOURCE_DIR}/"
    DESTINATION "${_MUKEI_QML_TEST_SOURCE_ROOT}"
    PATTERN "qmldir" EXCLUDE
    PATTERN "CMakeLists.txt" EXCLUDE
)
foreach(_singleton IN LISTS _MUKEI_QML_SINGLETONS)
    get_filename_component(_singleton_dir "${_singleton}" DIRECTORY)
    get_filename_component(_singleton_name "${_singleton}" NAME_WE)
    set(_singleton_qmldir
        "${_MUKEI_QML_TEST_SOURCE_ROOT}/${_singleton_dir}/qmldir"
    )
    file(APPEND "${_singleton_qmldir}"
        "singleton ${_singleton_name} 1.0 ${_singleton_name}.qml\n"
    )
endforeach()

qt_add_executable(tst_EventDispatcher
    tests/tst_EventDispatcher.cpp
)

target_compile_definitions(tst_EventDispatcher PRIVATE
    EVENT_DISPATCHER_QML_SOURCE_DIR="${_MUKEI_QML_TEST_SOURCE_ROOT}/tests"
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
    qml.qrc
)

target_compile_definitions(tst_QmlArchitecture PRIVATE
    QML_ARCHITECTURE_TEST_SOURCE_DIR="${_MUKEI_QML_TEST_SOURCE_ROOT}/tests"
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

qt_add_executable(tst_ModelStoreActivation
    tests/tst_ModelStoreActivation.cpp
)

target_compile_definitions(tst_ModelStoreActivation PRIVATE
    MODEL_STORE_ACTIVATION_QML_SOURCE_DIR="${_MUKEI_QML_TEST_SOURCE_ROOT}/tests"
)

target_link_libraries(tst_ModelStoreActivation PRIVATE
    Qt6::Core
    Qt6::Gui
    Qt6::Quick
    Qt6::Qml
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

add_test(NAME tst_ModelStoreActivation COMMAND tst_ModelStoreActivation -platform offscreen -input ${_MUKEI_QML_TEST_SOURCE_ROOT}/tests/tst_ModelStoreActivation.qml)
set_tests_properties(tst_ModelStoreActivation PROPERTIES ENVIRONMENT "QT_QPA_PLATFORM=offscreen")

add_test(NAME tst_TimelineModel COMMAND tst_TimelineModel)

add_test(NAME tst_EventDispatcher COMMAND tst_EventDispatcher -platform offscreen -input ${_MUKEI_QML_TEST_SOURCE_ROOT}/tests/tst_EventDispatcher.qml)
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
