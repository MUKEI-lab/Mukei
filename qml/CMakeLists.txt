cmake_minimum_required(VERSION 3.22)
project(MukeiQml VERSION 0.7.5 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)
set(QT_QUICK_CONTROLS_STYLE "Basic")
include(CTest)

option(MUKEI_USE_REAL_BRIDGE "Instantiate CXX-Qt bridge objects instead of local QML stubs" OFF)

# Ubuntu 24.04 ships Qt 6.4.x; that baseline is sufficient for the checked-in
# QML scaffold, QuickTest runners, and variable fonts used by CI smoke tests.
# Newer Qt builds remain supported, but CI must stay green on the distro toolchain.
find_package(Qt6 6.4 REQUIRED COMPONENTS
    Core Gui Quick Qml QuickControls2 QuickTest Widgets Svg
)

# Cargo build directory selection. Android builds still cross-compile to
# `aarch64-linux-android/release`; every other host (Linux/macOS desktop
# smoke-runs) uses the plain `release` directory. Both the .so and .dylib
# suffixes are probed so a macOS build picks up libmukei_bridge.dylib.
if(ANDROID)
    set(_MUKEI_BRIDGE_DEFAULT
        ${CMAKE_CURRENT_SOURCE_DIR}/../rust/target/aarch64-linux-android/release/libmukei_bridge.so
    )
elseif(APPLE)
    set(_MUKEI_BRIDGE_DEFAULT
        ${CMAKE_CURRENT_SOURCE_DIR}/../rust/target/release/libmukei_bridge.dylib
    )
else()
    set(_MUKEI_BRIDGE_DEFAULT
        ${CMAKE_CURRENT_SOURCE_DIR}/../rust/target/release/libmukei_bridge.so
    )
endif()
set(MUKEI_BRIDGE_LIB "${_MUKEI_BRIDGE_DEFAULT}"
    CACHE FILEPATH "Path to the Cargo-built mukei-bridge shared library"
)

add_executable(mukei
    main.cpp
    qml.qrc
)

set_source_files_properties(events/EventDispatcher.qml PROPERTIES QT_QML_SINGLETON_TYPE TRUE)

qt_add_qml_module(mukei
    URI com.mukei.app
    VERSION 1.0
    RESOURCE_PREFIX /
    QML_FILES
        MainWindow.qml
        events/EventDispatcher.qml
        theme/Theme.qml
        theme/Type.qml
        theme/Spacing.qml
        theme/Motion.qml
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
        components/StreamingCaret.qml
        components/ThinkingAccordion.qml
        components/ToastNotification.qml
        components/ToolResultCard.qml
        components/UserMessageBubble.qml
        screens/ChatScreen.qml
        screens/EmptyChatScreen.qml
        screens/ModelManagerScreen.qml
        screens/ModelPickerScreen.qml
        screens/RagRebuildPromptScreen.qml
        screens/SafeModeScreen.qml
        screens/SettingsScreen.qml
        screens/ToolResultExpandedScreen.qml
        screens/VerificationScreen.qml
        screens/WelcomeScreen.qml
)

target_link_libraries(mukei PRIVATE
    Qt6::Core
    Qt6::Gui
    Qt6::Quick
    Qt6::Qml
    Qt6::QuickControls2
    Qt6::Widgets
    Qt6::Svg
)

if(EXISTS ${MUKEI_BRIDGE_LIB})
    target_link_libraries(mukei PRIVATE ${MUKEI_BRIDGE_LIB})
elseif(MUKEI_USE_REAL_BRIDGE)
    message(FATAL_ERROR "MUKEI_USE_REAL_BRIDGE=ON but MUKEI_BRIDGE_LIB does not exist: ${MUKEI_BRIDGE_LIB}")
endif()

if(MUKEI_USE_REAL_BRIDGE)
    target_compile_definitions(mukei PRIVATE MUKEI_USE_REAL_BRIDGE=1)
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
    Qt6::QuickTest
)

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
