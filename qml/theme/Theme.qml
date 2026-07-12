pragma Singleton
import QtQuick

QtObject {
    id: theme

    enum Mode {
        DolceVita,
        Espresso,
        Taupe
    }

    component Palette: QtObject {
        property color background
        property color surface
        property color surfaceVariant
        property color surfaceFaint
        property color inkPrimary
        property color inkSecondary
        property color inkFaint
        property color accent
        property color accentSoft
        property color divider
    }

    property int mode: Theme.Mode.DolceVita
    property bool reduceMotion: false
    property bool highContrast: false
    property real scale: 1.0
    readonly property string scaleClass: scale > 1.5 ? "large" : "regular"

    readonly property Palette dv: Palette {
        background: "#D8CABD"
        surface: "#E8DDD0"
        surfaceVariant: "#C9B9A7"
        surfaceFaint: "#DFD3C6"
        inkPrimary: "#362417"
        inkSecondary: "#6B5D4F"
        inkFaint: "#9C8E80"
        accent: "#B87333"
        accentSoft: "#D49A6A"
        divider: "#BFAE9C"
    }

    readonly property Palette esp: Palette {
        background: "#362417"
        surface: "#4A3829"
        surfaceVariant: "#5C4736"
        surfaceFaint: "#403020"
        inkPrimary: "#EBE1D5"
        inkSecondary: "#A89888"
        inkFaint: "#7D6E60"
        accent: "#D4AF37"
        accentSoft: "#E5C66A"
        divider: "#604A37"
    }

    readonly property Palette tp: Palette {
        background: "#92817A"
        surface: "#A89888"
        surfaceVariant: "#B5A697"
        surfaceFaint: "#998880"
        inkPrimary: "#2A2420"
        inkSecondary: "#4F423A"
        inkFaint: "#6A5C52"
        accent: "#C17F3E"
        accentSoft: "#D49E68"
        divider: "#7E6F66"
    }

    readonly property Palette p: mode === Theme.Mode.DolceVita
                                 ? dv
                                 : mode === Theme.Mode.Espresso ? esp : tp
    readonly property color success: "#10B981"
    readonly property color warning: "#F59E0B"
    readonly property color error: "#EF4444"
    readonly property color overlay: Qt.rgba(0, 0, 0, 0.4)
    readonly property int radiusSm: 4
    readonly property int radiusMd: 8
    readonly property int radiusLg: 12
    readonly property int radiusXl: 16
    readonly property int radiusXxl: 24
}
