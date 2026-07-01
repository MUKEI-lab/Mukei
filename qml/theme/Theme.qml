pragma Singleton
import QtQuick
QtObject {
    readonly property QtObject p: QtObject {
        readonly property color background: "#1A1816"; readonly property color surface: "#242120"; readonly property color surfaceFaint: "#211F1D"; readonly property color surfaceVariant: "#2E2B29"
        readonly property color inkPrimary: "#F5F0E8"; readonly property color inkSecondary: "#A8A29E"; readonly property color inkFaint: "#6B6560"
        readonly property color accent: "#D48C46"; readonly property color accentMuted: "#8F5F35"; readonly property color success: "#8FAE6A"; readonly property color warning: "#D6A34D"; readonly property color danger: "#D96C5F"
        readonly property color focusRing: "#F0B35F"
    }
    readonly property real radiusSm: 8; readonly property real radiusMd: 12; readonly property real radiusLg: 20
}
