pragma Singleton
import QtQuick

QtObject {
    readonly property var enter: [0.16, 1, 0.3, 1]
    readonly property var exit: [0.7, 0, 0.84, 0]
    readonly property int immediateFeedback: 110
    readonly property int microTransition: 160
    readonly property int contentChange: 220
    readonly property int sheetDialog: 280
    readonly property int bubbleAppear: contentChange
    readonly property int modalEnter: 280
    readonly property int modalExit: 200
    readonly property int fullScreenEnter: 300
    readonly property int fullScreenExit: 220
    readonly property int themeCrossFade: 300
    readonly property int progressValue: 220
    readonly property int caretDoneSwap: 160
    readonly property int buttonPressTint: 100
    readonly property int keyboardInsetPush: 240
    readonly property int drawerOpen: 260
    readonly property int drawerClose: 220
    readonly property int toolCrossFade: 80
    readonly property int toolPulse: 1100
    readonly property int destructiveMorph: 250
    readonly property int destructiveTimeout: 4000
    readonly property int toastDismiss: 2000
    readonly property int skeletonMaxVisible: 1500
}
