import QtQuick
import QtTest
import "../theme"

// Locks every palette hex code in Theme.qml. A future refactor cannot
// silently drift the editorial-luxury colour system: the moment a hex
// changes, this test either has to be updated in the same PR (visible
// diff) or CI fails. Values are the ones documented in UXB v2.1.
TestCase {
    name: "ThemeHexLock"

    function test_dolce_vita_palette() {
        Theme.mode = Theme.Mode.DolceVita;
        compare(Theme.p.background.toString(),      "#d8cabd", "DolceVita background");
        compare(Theme.p.surface.toString(),         "#e8ddd0", "DolceVita surface");
        compare(Theme.p.surfaceVariant.toString(),  "#c9b9a7", "DolceVita surfaceVariant");
        compare(Theme.p.surfaceFaint.toString(),    "#dfd3c6", "DolceVita surfaceFaint");
        compare(Theme.p.inkPrimary.toString(),      "#362417", "DolceVita inkPrimary");
        compare(Theme.p.inkSecondary.toString(),    "#6b5d4f", "DolceVita inkSecondary");
        compare(Theme.p.inkFaint.toString(),        "#9c8e80", "DolceVita inkFaint");
        compare(Theme.p.accent.toString(),          "#b87333", "DolceVita accent");
        compare(Theme.p.accentSoft.toString(),      "#d49a6a", "DolceVita accentSoft");
        compare(Theme.p.divider.toString(),         "#bfae9c", "DolceVita divider");
    }

    function test_espresso_palette() {
        Theme.mode = Theme.Mode.Espresso;
        compare(Theme.p.background.toString(),      "#362417", "Espresso background");
        compare(Theme.p.inkPrimary.toString(),      "#ebe1d5", "Espresso inkPrimary");
        compare(Theme.p.accent.toString(),          "#d4af37", "Espresso accent");
    }

    function test_taupe_palette() {
        Theme.mode = Theme.Mode.Taupe;
        compare(Theme.p.background.toString(),      "#92817a", "Taupe background");
        compare(Theme.p.inkPrimary.toString(),      "#2a2420", "Taupe inkPrimary");
        compare(Theme.p.accent.toString(),          "#c17f3e", "Taupe accent");
    }

    function test_semantic_aliases_shared_across_modes() {
        compare(Theme.success.toString(), "#10b981", "success is Tailwind emerald-500");
        compare(Theme.warning.toString(), "#f59e0b", "warning is Tailwind amber-500");
        compare(Theme.error.toString(),   "#ef4444", "error is Tailwind red-500");
    }
}
