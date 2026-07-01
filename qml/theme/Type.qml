pragma Singleton
import QtQuick

QtObject {
    id: type

    property real fontScale: Math.max(0.85, Math.min(2.0, Theme.scale))
    readonly property bool compact: fontScale > 1.5

    function px(value) {
        return Math.round(value * fontScale)
    }

    function fontSpec(family, size, lineHeight, weight, italic) {
        return {
            "family": family,
            "pixelSize": px(size),
            "lineHeight": lineHeight,
            "weight": weight,
            "italic": italic === true
        }
    }

    function apply(textItem, token) {
        textItem.font.family = token.family
        textItem.font.pixelSize = token.pixelSize
        textItem.font.weight = token.weight
        textItem.font.italic = token.italic
        textItem.lineHeightMode = Text.ProportionalHeight
        textItem.lineHeight = token.lineHeight
    }

    readonly property var display: fontSpec("Playfair Display", 32, 1.20, Font.DemiBold, false)
    readonly property var h1: fontSpec("Playfair Display", 24, 1.30, Font.Medium, false)
    readonly property var h2: fontSpec("Playfair Display", 20, 1.35, Font.Medium, false)
    readonly property var h3: fontSpec("Inter", 18, 1.40, Font.DemiBold, false)
    readonly property var bodyAI: fontSpec("Merriweather", 16, 1.60, Font.Normal, false)
    readonly property var bodyAIItalic: fontSpec("Merriweather", 16, 1.60, Font.Normal, true)
    readonly property var bodyUI: fontSpec("Inter", 16, 1.50, Font.Normal, false)
    readonly property var bodySmall: fontSpec("Inter", 14, 1.50, Font.Normal, false)
    readonly property var bodySmallItalic: fontSpec("Merriweather", 14, 1.50, Font.Normal, true)
    readonly property var code: fontSpec("JetBrains Mono", 14, 1.50, Font.Normal, false)
    readonly property var caption: fontSpec("Inter", 12, 1.40, Font.Medium, false)
    readonly property var micro: fontSpec("Inter", 10, 1.30, Font.Medium, false)
}
