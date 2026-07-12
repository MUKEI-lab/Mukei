#!/usr/bin/env python3
"""Static architecture guard for the Mukei QML layer."""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REQUIRED_FILES = {
    "architecture/AppCoordinator.qml",
    "architecture/AppStateHub.qml",
    "architecture/IntentDispatcher.qml",
    "architecture/PresentationPolicy.qml",
    "architecture/SnapshotController.qml",
    "stores/ContractStore.qml",
    "stores/LifecycleStore.qml",
    "stores/CapabilityStore.qml",
    "stores/NavigationStore.qml",
    "stores/UiSessionStore.qml",
    "stores/ChatStore.qml",
    "stores/RecoveryStore.qml",
    "stores/ConversationStore.qml",
    "stores/OperationStore.qml",
    "stores/ErrorStore.qml",
    "stores/DiagnosticsStore.qml",
    "stores/AccessibilityStore.qml",
    "shell/AppShell.qml",
    "shell/RouterHost.qml",
    "screens/CompatibilityScreen.qml",
}

ALLOWED_BRIDGE_FILES = {
    "architecture/AppCoordinator.qml",
    "architecture/IntentDispatcher.qml",
    "events/EventDispatcher.qml",
    "MainWindow.qml",
}

FORBIDDEN_PATTERNS = {
    "direct_agent_access": re.compile(r"\bmukeiAgent\b"),
    "direct_bridge_access": re.compile(r"\bmukeiBridge\b"),
    "private_server_path": re.compile(r"/var/mukei"),
    "sqlite_in_qml": re.compile(r"[\"\']\s*(?:SELECT\s+.+?\s+FROM|INSERT\s+INTO|UPDATE\s+[A-Za-z0-9_]+\s+SET|DELETE\s+FROM)\b", re.IGNORECASE),
    "webview": re.compile(r"\bWebView\b"),
}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("qml_root", nargs="?", default="qml")
    args = parser.parse_args()
    root = Path(args.qml_root).resolve()

    errors: list[str] = []
    warnings: list[str] = []

    for rel in sorted(REQUIRED_FILES):
        if not (root / rel).is_file():
            errors.append(f"missing required architecture file: {rel}")

    for path in sorted(root.rglob("*.qml")):
        rel = path.relative_to(root).as_posix()
        text = path.read_text(encoding="utf-8", errors="replace")
        if rel.startswith("tests/"):
            continue
        for name, pattern in FORBIDDEN_PATTERNS.items():
            if not pattern.search(text):
                continue
            if name in {"direct_agent_access", "direct_bridge_access"} and rel in ALLOWED_BRIDGE_FILES:
                continue
            errors.append(f"{rel}: architecture violation: {name}")

        if rel.startswith("screens/") and "ListModel {" in text:
            warnings.append(f"{rel}: screen declares a local ListModel; prefer a feature store/native model")
        if rel.startswith("screens/") and re.search(r"onChunk_generated\s*\(", text):
            errors.append(f"{rel}: legacy direct chunk handler is forbidden")
        if rel.startswith("screens/") and re.search(r"\.send_message\s*\(", text):
            errors.append(f"{rel}: direct backend send is forbidden")

    chat_store = (root / "stores/ChatStore.qml")
    if chat_store.is_file():
        chat_text = chat_store.read_text(encoding="utf-8", errors="replace")
        if "mukeiTimelineModel" not in chat_text:
            errors.append("stores/ChatStore.qml: native timeline projection is required")
        if "chat_snapshot_json" not in chat_text:
            errors.append("stores/ChatStore.qml: durable snapshot hydration is required")


    contract_store = root / "stores/ContractStore.qml"
    coordinator = root / "architecture/AppCoordinator.qml"
    operation_store = root / "stores/OperationStore.qml"
    if contract_store.is_file():
        contract_text = contract_store.read_text(encoding="utf-8", errors="replace")
        if "ui_contract_snapshot_json" not in contract_text:
            errors.append("stores/ContractStore.qml: native compatibility handshake is required")
        if "qmlContractVersion" not in contract_text:
            errors.append("stores/ContractStore.qml: explicit QML contract version is required")
    if coordinator.is_file():
        coordinator_text = coordinator.read_text(encoding="utf-8", errors="replace")
        start_pos = coordinator_text.find("function start()")
        retry_pos = coordinator_text.find("function retryContractNegotiation()")
        if start_pos < 0 or retry_pos < 0 or retry_pos <= start_pos:
            errors.append("architecture/AppCoordinator.qml: start() is required")
        else:
            start_body = coordinator_text[start_pos:retry_pos]
            contract_pos = start_body.find("ContractStore.hydrate()")
            continue_pos = start_body.find("continueStartupAfterContract()")
            if contract_pos < 0 or continue_pos < 0 or contract_pos > continue_pos:
                errors.append("architecture/AppCoordinator.qml: contract negotiation must precede startup continuation")
    if operation_store.is_file():
        operation_text = operation_store.read_text(encoding="utf-8", errors="replace")
        if "operation_snapshot_json" not in operation_text:
            errors.append("stores/OperationStore.qml: native durable operation snapshot is required")

    print("Mukei QML Architecture Analysis")
    print("=" * 36)
    print(f"Errors: {len(errors)}")
    print(f"Warnings: {len(warnings)}")
    for issue in errors:
        print(f"ERROR: {issue}")
    for issue in warnings:
        print(f"WARNING: {issue}")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
