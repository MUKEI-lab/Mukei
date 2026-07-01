pragma Singleton
import QtQuick

QtObject {
    id: theme

    enum Mode {
        DolceVita,
        Espresso,
        Taupe
    }

    property int mode: Theme.Mode.DolceVita
    property bool reduceMotion: false
    property bool highContrast: false
    readonly property real scale: 1.0
    readonly property string scaleClass: scale > 1.5 ? "large" : "regular"

    readonly property QtObject dv: QtObject {
        readonly property color background: "#D8CABD"
        readonly property color surface: "#E8DDD0"
        readonly property color surfaceVariant: "#C9B9A7"
        readonly property color surfaceFaint: "#DFD3C6"
        readonly property color inkPrimary: "#362417"
        readonly property color inkSecondary: "#6B5D4F"
        readonly property color inkFaint: "#9C8E80"
        readonly property color accent: "#B87333"
        readonly property color accentSoft: "#D49A6A"
        readonly property color divider: "#BFAE9C"
    }

    readonly property QtObject esp: QtObject {
        readonly property color background: "#362417"
        readonly property color surface: "#4A3829"
        readonly property color surfaceVariant: "#5C4736"
        readonly property color surfaceFaint: "#403020"
        readonly property color inkPrimary: "#EBE1D5"
        readonly property color inkSecondary: "#A89888"
        readonly property color inkFaint: "#7D6E60"
        readonly property color accent: "#D4AF37"
        readonly property color accentSoft: "#E5C66A"
        readonly property color divider: "#604A37"
    }

    readonly property QtObject tp: QtObject {
        readonly property color background: "#92817A"
        readonly property color surface: "#A89888"
        readonly property color surfaceVariant: "#B5A697"
        readonly property color surfaceFaint: "#998880"
        readonly property color inkPrimary: "#2A2420"
        readonly property color inkSecondary: "#4F423A"
        readonly property color inkFaint: "#6A5C52"
        readonly property color accent: "#C17F3E"
        readonly property color accentSoft: "#D49E68"
        readonly property color divider: "#7E6F66"
    }

    readonly property QtObject p: mode === Theme.Mode.DolceVita ? dv : mode === Theme.Mode.Espresso ? esp : tp
    readonly property color success: "#10B981"
    readonly property color warning: "#F59E0B"
    readonly property color error: "#EF4444"
    readonly property color overlay: Qt.rgba(0, 0, 0, 0.4)
}
