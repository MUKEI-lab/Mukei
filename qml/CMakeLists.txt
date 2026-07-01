cmake_minimum_required(VERSION 3.21)
project(MukeiQml LANGUAGES CXX)

find_package(Qt6 6.5 REQUIRED COMPONENTS Core Gui Qml Quick QuickControls2)
qt_standard_project_setup(REQUIRES 6.5)

qt_add_executable(mukei-qml main.cpp)
qt_add_qml_module(mukei-qml
    URI com.mukei.app
    VERSION 1.0
    RESOURCE_PREFIX /
    QML_FILES
        MainWindow.qml
        theme/Theme.qml theme/Type.qml theme/Spacing.qml theme/Motion.qml
        components/PrimaryButton.qml components/SecondaryButton.qml components/GhostButton.qml components/IconButton.qml components/DestructiveButton.qml
        components/ChatComposer.qml components/UserMessageBubble.qml components/AIMessageBubble.qml components/ChatTimelineEvent.qml
        components/ToolResultCard.qml components/RAGChunkCard.qml components/LeftDrawer.qml components/ModalSheet.qml components/FullScreenModal.qml
        components/ProgressBar.qml components/Spinner.qml components/SkeletonLoader.qml components/StatusPill.qml components/StreamingCaret.qml
        components/ConfirmationDialog.qml components/DestructiveConfirmDialog.qml components/ToastNotification.qml components/MarkdownRenderer.qml
        components/CodeBlockComponent.qml components/ThinkingAccordion.qml components/CopyButton.qml components/HapticFeedback.qml components/FontLoader.qml components/NetworkBanner.qml
        screens/WelcomeScreen.qml screens/ModelPickerScreen.qml screens/VerificationScreen.qml screens/EmptyChatScreen.qml screens/ChatScreen.qml
        screens/ModelManagerScreen.qml screens/SettingsScreen.qml screens/SafeModeScreen.qml screens/RagRebuildPromptScreen.qml screens/ToolResultExpandedScreen.qml
    RESOURCES qml.qrc
)

target_link_libraries(mukei-qml PRIVATE Qt6::Core Qt6::Gui Qt6::Qml Qt6::Quick Qt6::QuickControls2)
