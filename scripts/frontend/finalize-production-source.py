#!/usr/bin/env python3
"""Finalize canonical production integration source.

This script is deliberately narrow and idempotent. It repairs only integration
seams proven by CI or physical-device testing:

* duplicate standalone-stub initialization completions;
* consistent camelCase/snake_case event signals for both stub bridge objects;
* Android model-directory validation for Qt's internal app-private files path.

The Android patch preserves the storage boundary. It accepts only canonical
Android application sandboxes (credential/device-protected internal storage,
legacy /data/data, adopted private storage, or Android/data app-specific files)
and continues rejecting shared Download/storage paths and symlink escapes.
"""
from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MAIN_CPP = ROOT / "qml/main.cpp"
BRIDGE_LIB_RS = ROOT / "rust/crates/mukei-bridge/src/lib.rs"

STUB_COMPLETION = '''                emitOperationLifecycle(context, true, QJsonObject{
                    {QStringLiteral("initialized"), true},
                    {QStringLiteral("runtime"), QStringLiteral("stub")}
                });'''

ANDROID_POLICY_OLD = '''    if is_android_app_specific_files_path(&canonical_dir) {
        return Ok(ValidatedModelDir {
            canonical_base: canonical_dir.clone(),
            canonical_dir,
        });
    }
'''

ANDROID_POLICY_NEW = '''    if cfg!(target_os = "android")
        && (is_android_app_specific_files_path(&canonical_dir)
            || is_android_internal_app_files_path(&canonical_dir))
    {
        return Ok(ValidatedModelDir {
            canonical_base: canonical_dir.clone(),
            canonical_dir,
        });
    }
'''

ANDROID_INTERNAL_HELPERS = r'''
fn is_android_package_component(value: &str) -> bool {
    let segments: Vec<&str> = value.split('.').collect();
    segments.len() >= 2
        && segments.iter().all(|segment| {
            !segment.is_empty()
                && segment
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_')
        })
}

fn is_android_internal_app_files_path(path: &std::path::Path) -> bool {
    let parts: Vec<String> = path
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => Some(part.to_string_lossy().to_string()),
            _ => None,
        })
        .collect();

    let standard_internal = parts.len() >= 5
        && parts[0] == "data"
        && matches!(parts[1].as_str(), "user" | "user_de")
        && parts[2].parse::<u32>().is_ok()
        && is_android_package_component(&parts[3])
        && parts[4] == "files";

    let legacy_internal = parts.len() >= 4
        && parts[0] == "data"
        && parts[1] == "data"
        && is_android_package_component(&parts[2])
        && parts[3] == "files";

    let adopted_internal = parts.len() >= 7
        && parts[0] == "mnt"
        && parts[1] == "expand"
        && !parts[2].is_empty()
        && matches!(parts[3].as_str(), "user" | "user_de")
        && parts[4].parse::<u32>().is_ok()
        && is_android_package_component(&parts[5])
        && parts[6] == "files";

    standard_internal || legacy_internal || adopted_internal
}
'''

ANDROID_INTERNAL_TEST = r'''
    #[test]
    fn android_model_dir_policy_accepts_internal_app_private_roots() {
        for path in [
            "/data/user/0/com.mukei.app/files/models",
            "/data/user_de/10/com.mukei.app/files/models",
            "/data/data/com.mukei.app/files/models",
            "/mnt/expand/01234567-89ab-cdef-0123-456789abcdef/user/0/com.mukei.app/files/models",
        ] {
            assert!(
                is_android_internal_app_files_path(std::path::Path::new(path)),
                "expected Android private path to be accepted: {path}"
            );
        }

        for path in [
            "/storage/emulated/0/Download/models",
            "/storage/emulated/0/data/com.mukei.app/files/models",
            "/data/local/tmp/com.mukei.app/files/models",
            "/data/user/not-a-user/com.mukei.app/files/models",
            "/data/user/0/com_mukei_app/files/models",
        ] {
            assert!(
                !is_android_internal_app_files_path(std::path::Path::new(path)),
                "expected non-private Android path to be rejected: {path}"
            );
        }
    }
'''


def collapse_stub_completions(text: str) -> str:
    while text.count(STUB_COMPLETION) > 1:
        text = text.replace(STUB_COMPLETION + "\n" + STUB_COMPLETION,
                            STUB_COMPLETION, 1)
    if text.count(STUB_COMPLETION) != 1:
        raise SystemExit(
            f"expected exactly one stub initialization completion; found {text.count(STUB_COMPLETION)}"
        )
    return text


def ensure_bridge_camel_signal(text: str) -> str:
    class_start = text.find("class MukeiBridgeStub final")
    class_end = text.find("class SafRegistryStub final", class_start)
    if class_start < 0 or class_end < 0:
        raise SystemExit("MukeiBridgeStub class boundaries not found")

    block = text[class_start:class_end]
    snake = "    void event_emitted(const QString &eventJson);"
    camel = "    void eventEmitted(const QString &eventJson);"
    if snake not in block:
        raise SystemExit("MukeiBridgeStub snake_case event signal missing")
    if camel not in block:
        block = block.replace(snake, snake + "\n" + camel, 1)
        text = text[:class_start] + block + text[class_end:]
    return text


def finalize_qml_cpp(text: str) -> str:
    text = collapse_stub_completions(text)
    text = ensure_bridge_camel_signal(text)

    bridge_start = text.find("class MukeiBridgeStub final")
    bridge_end = text.find("class SafRegistryStub final", bridge_start)
    bridge = text[bridge_start:bridge_end]
    required = [
        "void event_emitted(const QString &eventJson);",
        "void eventEmitted(const QString &eventJson);",
        "emit eventEmitted(eventJson);",
    ]
    for marker in required:
        if marker not in bridge:
            raise SystemExit(f"bridge event contract marker missing: {marker}")
    if text.count(STUB_COMPLETION) != 1:
        raise SystemExit("stub initialization completion is not unique")
    return text


def finalize_android_model_dir_policy(text: str) -> str:
    if "is_android_internal_app_files_path" not in text:
        helper_anchor = "\n#[cfg(any(debug_assertions, test))]\nfn safe_model_filename"
        if helper_anchor not in text:
            raise SystemExit("Android model-directory helper insertion anchor missing")
        text = text.replace(
            helper_anchor,
            ANDROID_INTERNAL_HELPERS + helper_anchor,
            1,
        )

    if ANDROID_POLICY_NEW not in text:
        if ANDROID_POLICY_OLD not in text:
            raise SystemExit("Android model-directory policy anchor missing")
        text = text.replace(ANDROID_POLICY_OLD, ANDROID_POLICY_NEW, 1)

    import_old = '''    use super::{
        is_android_app_specific_files_path, safe_model_filename, validate_model_dir_against_base,
        MukeiAgentRust,
    };'''
    import_new = '''    use super::{
        is_android_app_specific_files_path, is_android_internal_app_files_path,
        safe_model_filename, validate_model_dir_against_base, MukeiAgentRust,
    };'''
    if import_new not in text:
        if import_old not in text:
            raise SystemExit("model-directory test import anchor missing")
        text = text.replace(import_old, import_new, 1)

    if "fn android_model_dir_policy_accepts_internal_app_private_roots()" not in text:
        test_anchor = '''
    #[test]
    fn debug_custom_model_filename_must_be_simple_gguf() {'''
        if test_anchor not in text:
            raise SystemExit("Android model-directory test insertion anchor missing")
        text = text.replace(test_anchor, ANDROID_INTERNAL_TEST + test_anchor, 1)

    required = [
        'cfg!(target_os = "android")',
        "fn is_android_internal_app_files_path",
        'parts[0] == "data"',
        'matches!(parts[1].as_str(), "user" | "user_de")',
        "android_model_dir_policy_accepts_internal_app_private_roots",
        '"/data/user/0/com.mukei.app/files/models"',
        '"/storage/emulated/0/Download/models"',
    ]
    for marker in required:
        if marker not in text:
            raise SystemExit(f"Android model-directory policy marker missing: {marker}")
    return text


def main() -> int:
    main_cpp = finalize_qml_cpp(MAIN_CPP.read_text(encoding="utf-8"))
    bridge_rs = finalize_android_model_dir_policy(
        BRIDGE_LIB_RS.read_text(encoding="utf-8")
    )

    MAIN_CPP.write_text(main_cpp, encoding="utf-8")
    BRIDGE_LIB_RS.write_text(bridge_rs, encoding="utf-8")
    print("Canonical production source finalized")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
