# Mukei Bundled Fonts (SIL Open Font License 1.1)

Mukei ships **8 variable-axis TrueType fonts** (upright + italic per family)
covering the four typographic families the UX Brief (v2.1) mandates:

| Family              | File                                    | Axes            | License       | Upstream                                                |
| ------------------- | --------------------------------------- | --------------- | ------------- | ------------------------------------------------------- |
| Playfair Display    | `PlayfairDisplay-Variable.ttf`          | `wght`          | SIL OFL 1.1   | https://github.com/google/fonts/tree/main/ofl/playfairdisplay |
| Playfair Display It | `PlayfairDisplay-Italic-Variable.ttf`   | `wght`          | SIL OFL 1.1   | ditto                                                    |
| Merriweather        | `Merriweather-Variable.ttf`             | `opsz,wdth,wght`| SIL OFL 1.1   | https://github.com/google/fonts/tree/main/ofl/merriweather   |
| Merriweather It     | `Merriweather-Italic-Variable.ttf`      | `opsz,wdth,wght`| SIL OFL 1.1   | ditto                                                    |
| Inter               | `Inter-Variable.ttf`                    | `opsz,wght`     | SIL OFL 1.1   | https://github.com/google/fonts/tree/main/ofl/inter          |
| Inter Italic        | `Inter-Italic-Variable.ttf`             | `opsz,wght`     | SIL OFL 1.1   | ditto                                                    |
| JetBrains Mono      | `JetBrainsMono-Variable.ttf`            | `wght`          | SIL OFL 1.1   | https://github.com/google/fonts/tree/main/ofl/jetbrainsmono  |
| JetBrains Mono It   | `JetBrainsMono-Italic-Variable.ttf`     | `wght`          | SIL OFL 1.1   | ditto                                                    |

**Why variable fonts?** One file per family instead of five, all weights
(Regular / Medium / SemiBold / Bold …) reachable through `Font.weight` at
runtime. Qt 6.5+ resolves `family + wght` against the variation table
automatically. Result: ~11 MB total, half the file handles, no synthetic
bolding artifacts.

**Attribution.** Each family carries the SIL Open Font License 1.1
verbatim in its upstream directory (see the `OFL.txt` files linked above).
Redistribution here does not modify the fonts, so the reserved names
"Playfair Display", "Merriweather", "Inter", and "JetBrains Mono" remain
their upstream authors'.
