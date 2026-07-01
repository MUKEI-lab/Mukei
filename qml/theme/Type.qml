pragma Singleton
import QtQuick
QtObject {
    function f(family, px, weight) { return Qt.font({ family: family, pixelSize: px, weight: weight }) }
    readonly property font h1: f("Playfair Display", 32, Font.Bold); readonly property font h2: f("Playfair Display", 24, Font.DemiBold); readonly property font h3: f("Inter", 18, Font.DemiBold)
    readonly property font bodyAI: f("Merriweather", 16, Font.Normal); readonly property font bodyUI: f("Inter", 16, Font.Normal); readonly property font bodySmall: f("Inter", 14, Font.Normal)
    readonly property font code: f("JetBrains Mono", 14, Font.Normal); readonly property font caption: f("Inter", 12, Font.Medium); readonly property font micro: f("Inter", 10, Font.Medium)
}
