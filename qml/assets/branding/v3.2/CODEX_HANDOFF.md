# Mukei branding integration — Codex handoff

## Do not replace AndroidManifest.xml wholesale

Merge the resource overlay first:

`02_ANDROID_RESOURCE_OVERLAY/qml/android/res/`
→ repository `qml/android/res/`

Then patch the repository's Qt-compatible Android manifest:

- add `android:icon="@mipmap/ic_launcher"` to `<application>`
- add `android:roundIcon="@mipmap/ic_launcher_round"` to `<application>`
- point `android:theme` to the final Qt-compatible application theme

Preserve all Qt-required manifest placeholders and metadata from the installed
Qt 6.5.3 manifest template, including:

- mandatory `android.app.lib_name`
- Qt permission/feature insertion markers
- any generated application/activity metadata
- existing Java source files under `qml/android/src/`

Do not assume that `@style/AppTheme` exists. Inspect the generated Qt Android
project or installed Qt template, then merge `THEME_SNIPPETS.xml` into the
actual compatible theme.

## Required verification

1. Clean Android build from the exact source SHA.
2. `aapt2`/Gradle resource merge passes.
3. Merged manifest contains launcher icon, round icon, Qt metadata and app theme.
4. APK contains all `mipmap-*` resources and adaptive XML.
5. Android 8+ adaptive icon renders without clipping.
6. Android 13+ themed icon uses the monochrome layer.
7. Android 12+ splash shows espresso/copper mark on paper background.
8. Install and cold-launch smoke test passes.
