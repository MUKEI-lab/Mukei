#!/usr/bin/env python3
"""Static Android packaging and approved-branding checks for the APK-first path."""

from __future__ import annotations

import hashlib
import importlib.util
import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[2]
ANDROID = ROOT / "qml" / "android"
ANDROID_NS = "http://schemas.android.com/apk/res/android"
A = f"{{{ANDROID_NS}}}"
OVERLAY_PREFIX = PurePosixPath("02_ANDROID_RESOURCE_OVERLAY/qml/android/res")


def fail(message: str) -> "NoReturn":
    raise SystemExit(f"android preflight failed: {message}")


def parse_xml(path: Path) -> ET.Element:
    if not path.is_file():
        fail(f"required file is missing: {path.relative_to(ROOT)}")
    try:
        return ET.parse(path).getroot()
    except ET.ParseError as error:
        fail(f"invalid XML in {path.relative_to(ROOT)}: {error}")


def require_text(path: Path, needles: tuple[str, ...]) -> str:
    if not path.is_file():
        fail(f"required file is missing: {path.relative_to(ROOT)}")
    text = path.read_text(encoding="utf-8")
    for needle in needles:
        if needle not in text:
            fail(f"{path.relative_to(ROOT)} is missing contract token: {needle}")
    return text


def workspace_version() -> tuple[str, int]:
    cargo_toml = (ROOT / "rust" / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(
        r'^version\s*=\s*"([0-9]+)\.([0-9]+)\.([0-9]+)"',
        cargo_toml,
        re.MULTILINE,
    )
    if not match:
        fail("rust/Cargo.toml workspace version was not found")
    major, minor, patch = (int(part) for part in match.groups())
    if minor >= 100 or patch >= 100:
        fail("Android versionCode formula requires minor and patch below 100")
    return f"{major}.{minor}.{patch}", major * 10000 + minor * 100 + patch


def load_branding_module():
    module_path = ROOT / "scripts" / "android" / "prepare-branding.py"
    spec = importlib.util.spec_from_file_location("mukei_prepare_branding", module_path)
    if spec is None or spec.loader is None:
        fail("could not load scripts/android/prepare-branding.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def check_branding_bundle_and_overlay() -> None:
    branding = load_branding_module()
    _, manifest = branding.load_payload()
    branding.verify()

    committed_members = (
        "drawable/ic_launcher_foreground.xml",
        "drawable/ic_launcher_monochrome.xml",
        "drawable/mukei_splash_background.xml",
        "drawable/mukei_splash_icon.xml",
        "mipmap-anydpi-v26/ic_launcher.xml",
        "mipmap-anydpi-v26/ic_launcher_round.xml",
        "values/mukei_brand_colors.xml",
    )
    for relative in committed_members:
        member = str(OVERLAY_PREFIX / relative)
        destination = ANDROID / "res" / Path(relative)
        if member not in manifest:
            fail(f"approved branding manifest is missing {member}")
        expected_size, expected_hash = manifest[member]
        if not destination.is_file():
            fail(f"approved branding resource is missing: {destination.relative_to(ROOT)}")
        actual = destination.read_bytes()
        if len(actual) != expected_size or hashlib.sha256(actual).hexdigest() != expected_hash:
            fail(f"approved branding resource was modified: {destination.relative_to(ROOT)}")

    obsolete = (
        ROOT / "qml" / "assets" / "branding" / "mukei-app-icon.svg",
        ANDROID / "res" / "values" / "colors.xml",
        ANDROID / "res" / "mipmap-anydpi-v33" / "ic_launcher.xml",
        ANDROID / "res" / "mipmap-anydpi-v33" / "ic_launcher_round.xml",
    )
    for path in obsolete:
        if path.exists():
            fail(f"obsolete placeholder branding remains: {path.relative_to(ROOT)}")


def check_adaptive_icon(path: Path) -> None:
    adaptive = parse_xml(path)
    if adaptive.tag != "adaptive-icon":
        fail(f"{path.relative_to(ROOT)} is not an adaptive-icon")
    expected = {
        "background": "@color/ic_launcher_background",
        "foreground": "@drawable/ic_launcher_foreground",
        "monochrome": "@drawable/ic_launcher_monochrome",
    }
    for child_name, drawable in expected.items():
        child = adaptive.find(child_name)
        if child is None or child.get(A + "drawable") != drawable:
            fail(f"{path.relative_to(ROOT)} has invalid {child_name} reference")


def style_items(path: Path) -> dict[str, str]:
    resources = parse_xml(path)
    style = next(
        (node for node in resources.findall("style") if node.get("name") == "MukeiAppTheme"),
        None,
    )
    if style is None:
        fail(f"{path.relative_to(ROOT)} does not declare MukeiAppTheme")
    return {node.get("name", ""): (node.text or "").strip() for node in style.findall("item")}


def check_manifest_launcher_and_splash() -> None:
    manifest_path = ANDROID / "AndroidManifest.xml"
    require_text(
        manifest_path,
        (
            "<!-- %%INSERT_PERMISSIONS -->",
            "<!-- %%INSERT_FEATURES -->",
            "-- %%INSERT_APP_LIB_NAME%% --",
            "-- %%INSERT_APP_ARGUMENTS%% --",
        ),
    )
    manifest = parse_xml(manifest_path)
    version_name, version_code = workspace_version()
    if manifest.get(A + "versionName") != version_name:
        fail("Android versionName must match rust/Cargo.toml workspace version")
    if manifest.get(A + "versionCode") != str(version_code):
        fail("Android versionCode must use major*10000 + minor*100 + patch")
    if manifest.get(A + "installLocation") != "auto":
        fail("Qt-compatible manifest installLocation must remain auto")

    uses_sdk = manifest.find("uses-sdk")
    if uses_sdk is None:
        fail("AndroidManifest.xml has no uses-sdk element")
    if uses_sdk.get(A + "minSdkVersion") != "29":
        fail("minSdkVersion must remain 29 for the APK-first target")
    if uses_sdk.get(A + "targetSdkVersion") != "35":
        fail("targetSdkVersion must remain 35")

    supports = manifest.find("supports-screens")
    if supports is None or any(
        supports.get(A + attribute) != "true"
        for attribute in ("anyDensity", "largeScreens", "normalScreens", "smallScreens")
    ):
        fail("Qt-compatible supports-screens contract is incomplete")

    application = manifest.find("application")
    if application is None:
        fail("AndroidManifest.xml has no application element")
    expected_application = {
        "name": "org.qtproject.qt.android.bindings.QtApplication",
        "icon": "@mipmap/ic_launcher",
        "roundIcon": "@mipmap/ic_launcher_round",
        "theme": "@style/MukeiAppTheme",
        "hardwareAccelerated": "true",
        "requestLegacyExternalStorage": "false",
        "allowNativeHeapPointerTagging": "false",
        "allowBackup": "false",
        "fullBackupOnly": "false",
    }
    for attribute, expected in expected_application.items():
        if application.get(A + attribute) != expected:
            fail(f"application {attribute} must equal {expected}")

    activity = application.find("activity")
    expected_activity = {
        "name": "org.qtproject.qt.android.bindings.QtActivity",
        "theme": "@style/MukeiAppTheme",
        "launchMode": "singleTop",
        "screenOrientation": "unspecified",
        "exported": "true",
    }
    if activity is None:
        fail("QtActivity launcher declaration is missing")
    for attribute, expected in expected_activity.items():
        if activity.get(A + attribute) != expected:
            fail(f"QtActivity {attribute} must equal {expected}")
    required_config_changes = {
        "orientation", "uiMode", "screenLayout", "screenSize", "smallestScreenSize",
        "layoutDirection", "locale", "fontScale", "keyboard", "keyboardHidden",
        "navigation", "mcc", "mnc", "density",
    }
    if set((activity.get(A + "configChanges") or "").split("|")) != required_config_changes:
        fail("QtActivity configChanges do not match the Qt 6.5.3 template contract")

    metadata = {node.get(A + "name"): node for node in activity.findall("meta-data")}
    expected_values = {
        "android.app.lib_name": "-- %%INSERT_APP_LIB_NAME%% --",
        "android.app.arguments": "-- %%INSERT_APP_ARGUMENTS%% --",
        "android.app.extract_android_style": "minimal",
    }
    for name, value in expected_values.items():
        node = metadata.get(name)
        if node is None or node.get(A + "value") != value:
            fail(f"mandatory Qt metadata is missing or invalid: {name}")
    splash = metadata.get("android.app.splash_screen_drawable")
    if splash is None or splash.get(A + "resource") != "@drawable/mukei_splash_background":
        fail("Qt splash metadata must reference the approved Mukei splash background")

    for resource_name in ("ic_launcher", "ic_launcher_round"):
        check_adaptive_icon(ANDROID / "res" / "mipmap-anydpi-v26" / f"{resource_name}.xml")
    for drawable in (
        "ic_launcher_foreground.xml",
        "ic_launcher_monochrome.xml",
        "mukei_splash_background.xml",
        "mukei_splash_icon.xml",
    ):
        parse_xml(ANDROID / "res" / "drawable" / drawable)

    colors = parse_xml(ANDROID / "res" / "values" / "mukei_brand_colors.xml")
    colors_by_name = {
        node.get("name"): (node.text or "").strip().upper() for node in colors.findall("color")
    }
    if colors_by_name.get("ic_launcher_background") != "#2B211A":
        fail("approved espresso launcher background changed")
    if colors_by_name.get("mukei_splash_background") != "#F1E8DC":
        fail("approved paper splash background changed")

    required_base = {
        "android:windowBackground": "@drawable/mukei_splash_background",
        "android:windowLightStatusBar": "true",
        "android:statusBarColor": "@color/mukei_splash_background",
        "android:navigationBarColor": "@color/mukei_splash_background",
    }
    base_items = style_items(ANDROID / "res" / "values" / "styles.xml")
    for name, value in required_base.items():
        if base_items.get(name) != value:
            fail(f"base MukeiAppTheme has invalid {name}")

    required_v31 = {
        **required_base,
        "android:windowSplashScreenBackground": "@color/mukei_splash_background",
        "android:windowSplashScreenAnimatedIcon": "@drawable/mukei_splash_icon",
        "android:windowSplashScreenIconBackgroundColor": "@android:color/transparent",
    }
    v31_items = style_items(ANDROID / "res" / "values-v31" / "styles.xml")
    for name, value in required_v31.items():
        if v31_items.get(name) != value:
            fail(f"Android 12+ MukeiAppTheme has invalid {name}")


def check_qml_assets() -> None:
    qrc = parse_xml(ROOT / "qml" / "qml.qrc")
    declared_files = qrc.findall(".//file")
    if len(declared_files) < 35:
        fail("qml.qrc unexpectedly lost registered fonts or UI icons")
    for file_node in declared_files:
        if not file_node.text:
            fail("qml.qrc contains an empty file entry")
        asset_path = ROOT / "qml" / file_node.text.strip()
        if not asset_path.is_file():
            fail(f"qml.qrc references a missing asset: {file_node.text.strip()}")


def check_build_contract() -> None:
    require_text(
        ROOT / "scripts" / "android" / "build-apk.sh",
        (
            'readonly ABI="arm64-v8a"',
            'readonly RUST_TARGET="aarch64-linux-android"',
            'prepare-branding.py" verify',
            'prepare-branding.py" materialize',
            'prepare-branding.py" cleanup',
            "--profile android-release",
            '--features "shipping_native,android_keystore,runtime_hardening"',
            "MukeiAndroidApkInitialCache.cmake",
            "-DQT_ANDROID_BUILD_ALL_ABIS=OFF",
            "--target apk",
            'bash "${SCRIPT_DIR}/validate-apk.sh"',
        ),
    )
    require_text(
        ROOT / "scripts" / "android" / "validate-apk.sh",
        (
            "lib/arm64-v8a/libmukei_llama_native.so",
            "unexpected ABI packaged in APK-first artifact",
            "unzip -tqq",
        ),
    )
    require_text(
        ROOT / "qml" / "cmake" / "MukeiAndroidApkInitialCache.cmake",
        (
            "ANDROID_ABI=arm64-v8a",
            "aarch64-linux-android/android-release/libmukei_bridge.a",
            "prebuilt/arm64-v8a/libmukei_llama_native.so",
            "set(MUKEI_USE_REAL_BRIDGE ON",
            "set(MUKEI_USE_NATIVE_INFERENCE ON",
        ),
    )


def main() -> int:
    check_branding_bundle_and_overlay()
    check_manifest_launcher_and_splash()
    check_qml_assets()
    check_build_contract()
    print("Android APK preflight passed")
    print("  Mukei branding v3.2: exact payload verified")
    print("  launcher and splash resources: complete")
    print("  Qt 6.5.3 manifest contract: complete")
    print("  version metadata: synchronized")
    print("  QML asset references: complete")
    print("  ABI contract: arm64-v8a only")
    print("  Cargo profile: android-release")
    return 0


if __name__ == "__main__":
    sys.exit(main())
