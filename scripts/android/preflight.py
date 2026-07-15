#!/usr/bin/env python3
"""Static Android packaging contract checks for the APK-first release path."""

from __future__ import annotations

import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
ANDROID = ROOT / "qml" / "android"
ANDROID_NS = "http://schemas.android.com/apk/res/android"
A = f"{{{ANDROID_NS}}}"


def fail(message: str) -> None:
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


def check_adaptive_icon(path: Path, *, include_monochrome: bool) -> None:
    adaptive = parse_xml(path)
    if adaptive.tag != "adaptive-icon":
        fail(f"{path.relative_to(ROOT)} is not an adaptive-icon")

    expected = {
        "background": "@color/mukei_launcher_background",
        "foreground": "@drawable/ic_launcher_foreground",
    }
    if include_monochrome:
        expected["monochrome"] = "@drawable/ic_launcher_monochrome"

    for child_name, drawable in expected.items():
        child = adaptive.find(child_name)
        if child is None or child.get(A + "drawable") != drawable:
            fail(f"{path.relative_to(ROOT)} has invalid {child_name} reference")

    monochrome = adaptive.find("monochrome")
    if include_monochrome and monochrome is None:
        fail(f"{path.relative_to(ROOT)} must declare a monochrome layer")
    if not include_monochrome and monochrome is not None:
        fail(f"{path.relative_to(ROOT)} must keep the API 26 adaptive-icon schema")


def check_manifest_and_launcher() -> None:
    manifest = parse_xml(ANDROID / "AndroidManifest.xml")
    version_name, version_code = workspace_version()
    if manifest.get(A + "versionName") != version_name:
        fail("Android versionName must match rust/Cargo.toml workspace version")
    if manifest.get(A + "versionCode") != str(version_code):
        fail("Android versionCode must use major*10000 + minor*100 + patch")

    uses_sdk = manifest.find("uses-sdk")
    if uses_sdk is None:
        fail("AndroidManifest.xml has no uses-sdk element")
    if uses_sdk.get(A + "minSdkVersion") != "29":
        fail("minSdkVersion must remain 29 for the APK-first target")
    if uses_sdk.get(A + "targetSdkVersion") != "35":
        fail("targetSdkVersion must remain 35")

    application = manifest.find("application")
    if application is None:
        fail("AndroidManifest.xml has no application element")
    if application.get(A + "icon") != "@mipmap/ic_launcher":
        fail("application icon must reference @mipmap/ic_launcher")
    if application.get(A + "roundIcon") != "@mipmap/ic_launcher_round":
        fail("application roundIcon must reference @mipmap/ic_launcher_round")

    for resource_name in ("ic_launcher", "ic_launcher_round"):
        check_adaptive_icon(
            ANDROID / "res" / "mipmap-anydpi-v26" / f"{resource_name}.xml",
            include_monochrome=False,
        )
        check_adaptive_icon(
            ANDROID / "res" / "mipmap-anydpi-v33" / f"{resource_name}.xml",
            include_monochrome=True,
        )

    parse_xml(ANDROID / "res" / "drawable" / "ic_launcher_foreground.xml")
    parse_xml(ANDROID / "res" / "drawable" / "ic_launcher_monochrome.xml")
    colors = parse_xml(ANDROID / "res" / "values" / "colors.xml")
    if not any(node.get("name") == "mukei_launcher_background" for node in colors.findall("color")):
        fail("launcher background color is not declared")

    master_logo = parse_xml(ROOT / "qml" / "assets" / "branding" / "mukei-app-icon.svg")
    if not master_logo.tag.endswith("svg"):
        fail("canonical app icon is not an SVG document")


def check_qml_assets() -> None:
    qrc_path = ROOT / "qml" / "qml.qrc"
    qrc = parse_xml(qrc_path)
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
            "--profile android-release",
            '--features "shipping_native,android_keystore,runtime_hardening"',
            "MukeiAndroidApkInitialCache.cmake",
            "-DQT_ANDROID_BUILD_ALL_ABIS=OFF",
            '--target apk',
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
            'ANDROID_ABI=arm64-v8a',
            "aarch64-linux-android/android-release/libmukei_bridge.a",
            "prebuilt/arm64-v8a/libmukei_llama_native.so",
            "set(MUKEI_USE_REAL_BRIDGE ON",
            "set(MUKEI_USE_NATIVE_INFERENCE ON",
        ),
    )


def main() -> int:
    check_manifest_and_launcher()
    check_qml_assets()
    check_build_contract()
    print("Android APK preflight passed")
    print("  launcher resources: complete")
    print("  version metadata: synchronized")
    print("  QML asset references: complete")
    print("  ABI contract: arm64-v8a only")
    print("  Cargo profile: android-release")
    return 0


if __name__ == "__main__":
    sys.exit(main())
