cmake_minimum_required(VERSION 3.22)
project(MukeiQml VERSION 0.7.5 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)
set(QT_QUICK_CONTROLS_STYLE "Basic")

find_package(Qt6 6.5 REQUIRED COMPONENTS
    Core Gui Quick Qml QuickControls2 Widgets Svg
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

qt_add_qml_module(mukei
    URI com.mukei.app
    VERSION 1.0
    RESOURCE_PREFIX /
    QML_FILES
        MainWindow.qml
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
endif()

if(ANDROID)
    set_property(TARGET mukei PROPERTY QT_ANDROID_MIN_SDK_VERSION 29)
    set_property(TARGET mukei PROPERTY QT_ANDROID_TARGET_SDK_VERSION 35)
    set_property(TARGET mukei PROPERTY QT_ANDROID_PACKAGE_SOURCE_DIR
        ${CMAKE_CURRENT_SOURCE_DIR}/android
    )
endif()
