import QtQuick
import QtTest
import "../theme"

// Validates the Type singleton's fontSpec structure and fontScale clamping.
// UXB v2.1 pins the pixel sizes for display/h1/h2/h3/bodyAI/bodyUI/bodySmall/
// code/caption/micro; regressing any of them breaks the editorial rhythm.
TestCase {
    name: "TypeScale"

    function test_fontScale_is_clamped() {
        // Below floor
        Theme.scale = 0.1;
        verify(Type.fontScale >= 0.85, "fontScale must clamp to >= 0.85 (was " + Type.fontScale + ")");
        // Above ceiling
        Theme.scale = 5.0;
        verify(Type.fontScale <= 2.0, "fontScale must clamp to <= 2.0 (was " + Type.fontScale + ")");
        // Reset
        Theme.scale = 1.0;
    }

    function test_px_rounds_and_scales() {
        Theme.scale = 1.0;
        compare(Type.px(16), 16, "px() at scale 1.0 returns integer input");
        Theme.scale = 1.5;
        compare(Type.px(16), 24, "px(16) at scale 1.5 equals 24");
        Theme.scale = 1.0;
    }

    function test_display_h1_h2_h3_sizes_lock() {
        Theme.scale = 1.0;
        compare(Type.display.pixelSize, 32, "display pixelSize");
        compare(Type.h1.pixelSize,      24, "h1 pixelSize");
        compare(Type.h2.pixelSize,      20, "h2 pixelSize");
        compare(Type.h3.pixelSize,      18, "h3 pixelSize");
    }

    function test_body_and_code_sizes_lock() {
        Theme.scale = 1.0;
        compare(Type.bodyAI.pixelSize,    16, "bodyAI pixelSize");
        compare(Type.bodyUI.pixelSize,    16, "bodyUI pixelSize");
        compare(Type.bodySmall.pixelSize, 14, "bodySmall pixelSize");
        compare(Type.code.pixelSize,      14, "code pixelSize");
        compare(Type.caption.pixelSize,   12, "caption pixelSize");
        compare(Type.micro.pixelSize,     10, "micro pixelSize");
    }

    function test_families_match_UXB() {
        compare(Type.display.family,   "Playfair Display",  "display family");
        compare(Type.h1.family,        "Playfair Display",  "h1 family");
        compare(Type.h3.family,        "Inter",             "h3 family");
        compare(Type.bodyAI.family,    "Merriweather",      "bodyAI family");
        compare(Type.bodyUI.family,    "Inter",             "bodyUI family");
        compare(Type.code.family,      "JetBrains Mono",    "code family");
    }

    function test_bodyAIItalic_is_italic() {
        verify(Type.bodyAIItalic.italic === true, "bodyAIItalic italic flag");
        verify(Type.bodyAI.italic === false,       "bodyAI is upright");
    }
}
