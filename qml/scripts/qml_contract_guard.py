#!/usr/bin/env python3
"""Cross-check the frozen QML/Rust architecture contract."""
from __future__ import annotations

import re
import sys
from pathlib import Path


def require(condition: bool, message: str, errors: list[str]) -> None:
    if not condition:
        errors.append(message)


def capture(pattern: str, text: str, label: str, errors: list[str]) -> int | None:
    match = re.search(pattern, text)
    if not match:
        errors.append(f"missing {label}")
        return None
    return int(match.group(1))


def main() -> int:
    qml_root = Path(sys.argv[1] if len(sys.argv) > 1 else "qml").resolve()
    project_root = qml_root.parent
    errors: list[str] = []

    contract_qml = (qml_root / "stores/ContractStore.qml").read_text(encoding="utf-8")
    core_contract = (project_root / "rust/crates/mukei-core/src/ui_contract.rs").read_text(encoding="utf-8")
    bridge = (project_root / "rust/crates/mukei-bridge/src/lib.rs").read_text(encoding="utf-8")
    stub = (qml_root / "main.cpp").read_text(encoding="utf-8")
    cmake = (qml_root / "CMakeLists.txt").read_text(encoding="utf-8")
    coordinator = (qml_root / "architecture/AppCoordinator.qml").read_text(encoding="utf-8")
    operation_store = (qml_root / "stores/OperationStore.qml").read_text(encoding="utf-8")

    qml_version = capture(r"qmlContractVersion:\s*(\d+)", contract_qml, "QML contract version", errors)
    rust_version = capture(r"UI_CONTRACT_VERSION:\s*u32\s*=\s*(\d+)", core_contract, "Rust contract version", errors)
    min_version = capture(r"MIN_QML_CONTRACT_VERSION:\s*u32\s*=\s*(\d+)", core_contract, "minimum QML version", errors)
    max_version = capture(r"MAX_QML_CONTRACT_VERSION:\s*u32\s*=\s*(\d+)", core_contract, "maximum QML version", errors)

    if None not in (qml_version, rust_version, min_version, max_version):
        require(min_version <= qml_version <= max_version,
                f"QML contract {qml_version} is outside Rust range {min_version}-{max_version}", errors)
        require(rust_version == qml_version,
                f"frozen baseline mismatch: Rust={rust_version}, QML={qml_version}", errors)

    for method in ("ui_contract_snapshot_json", "operation_snapshot_json"):
        require(f"fn {method}" in bridge, f"bridge declaration missing {method}", errors)
        require(f"pub fn {method}" in bridge, f"bridge implementation missing {method}", errors)
        require(method in stub, f"desktop stub missing {method}", errors)

    for rel in ("stores/ContractStore.qml", "screens/CompatibilityScreen.qml"):
        require(rel in cmake, f"CMake QML module missing {rel}", errors)

    start_pos = coordinator.find("function start()")
    retry_pos = coordinator.find("function retryContractNegotiation()")
    require(start_pos >= 0 and retry_pos > start_pos, "AppCoordinator.start() is missing", errors)
    if start_pos >= 0 and retry_pos > start_pos:
        start_body = coordinator[start_pos:retry_pos]
        contract_pos = start_body.find("ContractStore.hydrate()")
        continue_pos = start_body.find("continueStartupAfterContract()")
        require(contract_pos >= 0 and continue_pos >= 0 and contract_pos < continue_pos,
                "contract negotiation does not precede startup continuation", errors)


    require('operationId: "download:" + (job.modelId || job.jobId)' in operation_store,
            "download operation fallback identity must prefer model_id to match live events", errors)
    require('format!("download:{operation_identity}")' in bridge,
            "native operation snapshot must use stable download identity", errors)

    require("let chunk = self.chunk_bytes.max(1);\n        let chunk" not in
            (project_root / "rust/crates/mukei-core/src/engine/llama_wrapper.rs").read_text(encoding="utf-8"),
            "duplicate mock-inference chunk binding returned", errors)

    print("Mukei QML/Rust Contract Guard")
    print("=" * 34)
    print(f"Errors: {len(errors)}")
    for error in errors:
        print(f"ERROR: {error}")
    return 1 if errors else 0


if __name__ == "__main__":
    raise SystemExit(main())
