#!/usr/bin/env python3
"""Enforce Mukei's narrowly-scoped OpenSSL dependency provenance.

Mukei uses rustls for network TLS. The only approved OpenSSL presence is the
low-level `openssl-sys` backend pulled by rusqlite's vendored SQLCipher feature
for local at-rest database encryption.

This guard validates the *resolved* Cargo graph for every shipping target listed
in deny.toml. It intentionally fails closed when the graph shape changes so a
new OpenSSL consumer cannot silently enter the release dependency surface.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tomllib
from collections import defaultdict
from pathlib import Path
from typing import Any, Iterable

RUST_ROOT = Path(__file__).resolve().parents[1]
REPO_ROOT = RUST_ROOT.parent
DENY_CONFIG = REPO_ROOT / "deny.toml"
CORE_MANIFEST = RUST_ROOT / "crates" / "mukei-core" / "Cargo.toml"
BRIDGE_MANIFEST = RUST_ROOT / "crates" / "mukei-bridge" / "Cargo.toml"

SQLCIPHER_RUSQLITE_FEATURE = "bundled-sqlcipher-vendored-openssl"
CORE_SQLCIPHER_EDGE = f"rusqlite/{SQLCIPHER_RUSQLITE_FEATURE}"
APPROVED_OPENSSL_CHAIN_SUFFIX = ("rusqlite", "libsqlite3-sys", "openssl-sys")
FORBIDDEN_HIGH_LEVEL_TLS_CRATES = {"openssl", "native-tls"}
MAX_PROVENANCE_PATHS_PER_TARGET = 256


class PolicyError(RuntimeError):
    """Raised when the resolved dependency graph violates policy."""


def load_toml(path: Path) -> dict[str, Any]:
    try:
        return tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as exc:
        raise PolicyError(f"unable to read {path}: {exc}") from exc


def shipping_targets() -> list[str]:
    config = load_toml(DENY_CONFIG)
    targets = config.get("graph", {}).get("targets", [])
    if not isinstance(targets, list) or not targets or not all(isinstance(t, str) and t for t in targets):
        raise PolicyError("deny.toml [graph].targets must contain at least one non-empty target triple")
    if len(targets) != len(set(targets)):
        raise PolicyError("deny.toml [graph].targets contains duplicate target triples")
    return targets


def verify_manifest_intent() -> None:
    core = load_toml(CORE_MANIFEST)
    bridge = load_toml(BRIDGE_MANIFEST)

    core_sqlcipher = core.get("features", {}).get("sqlcipher", [])
    if CORE_SQLCIPHER_EDGE not in core_sqlcipher:
        raise PolicyError(
            "mukei-core/sqlcipher must explicitly enable "
            f"{CORE_SQLCIPHER_EDGE!r}; refusing an implicit or system-OpenSSL SQLCipher path"
        )

    bridge_features = bridge.get("features", {})
    bridge_sqlcipher = bridge_features.get("sqlcipher", [])
    if "mukei-core/sqlcipher" not in bridge_sqlcipher:
        raise PolicyError("mukei-bridge/sqlcipher must forward to mukei-core/sqlcipher")

    bridge_default = bridge_features.get("default", [])
    if "sqlcipher" not in bridge_default:
        raise PolicyError("mukei-bridge default features must include sqlcipher for the shipped encrypted database path")


def cargo_metadata(target: str) -> dict[str, Any]:
    command = [
        "cargo",
        "metadata",
        "--format-version",
        "1",
        "--locked",
        "--all-features",
        "--filter-platform",
        target,
    ]
    result = subprocess.run(
        command,
        cwd=RUST_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip() or f"exit status {result.returncode}"
        raise PolicyError(f"cargo metadata failed for {target}: {detail}")
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise PolicyError(f"cargo metadata returned invalid JSON for {target}: {exc}") from exc


def package_maps(metadata: dict[str, Any]) -> tuple[dict[str, dict[str, Any]], dict[str, list[str]]]:
    packages_by_id: dict[str, dict[str, Any]] = {}
    ids_by_name: dict[str, list[str]] = defaultdict(list)
    for package in metadata.get("packages", []):
        package_id = package.get("id")
        name = package.get("name")
        if not isinstance(package_id, str) or not isinstance(name, str):
            raise PolicyError("cargo metadata package entry is missing a string id/name")
        packages_by_id[package_id] = package
        ids_by_name[name].append(package_id)
    return packages_by_id, dict(ids_by_name)


def single_package_id(ids_by_name: dict[str, list[str]], name: str, target: str) -> str:
    ids = ids_by_name.get(name, [])
    if len(ids) != 1:
        raise PolicyError(
            f"{target}: expected exactly one resolved {name!r} package, found {len(ids)}; "
            "dependency provenance requires explicit review when versions split or disappear"
        )
    return ids[0]


def non_dev_graph(metadata: dict[str, Any]) -> tuple[dict[str, set[str]], dict[str, set[str]], dict[str, set[str]]]:
    resolve = metadata.get("resolve")
    if not isinstance(resolve, dict):
        raise PolicyError("cargo metadata did not include a resolved dependency graph")

    adjacency: dict[str, set[str]] = defaultdict(set)
    reverse: dict[str, set[str]] = defaultdict(set)
    features: dict[str, set[str]] = {}

    for node in resolve.get("nodes", []):
        node_id = node.get("id")
        if not isinstance(node_id, str):
            raise PolicyError("cargo metadata resolve node is missing an id")
        features[node_id] = {feature for feature in node.get("features", []) if isinstance(feature, str)}
        for dep in node.get("deps", []):
            dep_id = dep.get("pkg")
            if not isinstance(dep_id, str):
                continue
            dep_kinds = dep.get("dep_kinds") or [{"kind": None}]
            # Production provenance ignores dev-only edges but includes normal and
            # build dependencies. --filter-platform has already removed edges for
            # unrelated target cfgs.
            if not any(kind.get("kind") != "dev" for kind in dep_kinds if isinstance(kind, dict)):
                continue
            adjacency[node_id].add(dep_id)
            reverse[dep_id].add(node_id)

    return dict(adjacency), dict(reverse), features


def package_name(packages_by_id: dict[str, dict[str, Any]], package_id: str) -> str:
    package = packages_by_id.get(package_id)
    if package is None:
        raise PolicyError(f"resolved graph references unknown package id {package_id!r}")
    return str(package["name"])


def registry_source_is_crates_io(package: dict[str, Any]) -> bool:
    source = package.get("source")
    return isinstance(source, str) and source.startswith("registry+") and "crates.io-index" in source


def ancestors_of(target_id: str, reverse: dict[str, set[str]]) -> set[str]:
    ancestors = {target_id}
    stack = [target_id]
    while stack:
        current = stack.pop()
        for parent in reverse.get(current, set()):
            if parent not in ancestors:
                ancestors.add(parent)
                stack.append(parent)
    return ancestors


def paths_to_target(
    start_id: str,
    target_id: str,
    adjacency: dict[str, set[str]],
    can_reach_target: set[str],
) -> list[list[str]]:
    paths: list[list[str]] = []
    stack: list[tuple[str, list[str]]] = [(start_id, [start_id])]

    while stack:
        current, path = stack.pop()
        if current == target_id:
            paths.append(path)
            if len(paths) > MAX_PROVENANCE_PATHS_PER_TARGET:
                raise PolicyError(
                    "OpenSSL provenance graph expanded beyond the reviewed path bound; "
                    "dependency policy requires explicit re-review"
                )
            continue
        for child in sorted(adjacency.get(current, set())):
            if child not in can_reach_target or child in path:
                continue
            stack.append((child, [*path, child]))

    return paths


def assert_exact_parent(
    target: str,
    child_name: str,
    child_id: str,
    expected_parent_name: str,
    expected_parent_id: str,
    reverse: dict[str, set[str]],
    packages_by_id: dict[str, dict[str, Any]],
) -> None:
    actual_parent_ids = reverse.get(child_id, set())
    if actual_parent_ids != {expected_parent_id}:
        actual = sorted(package_name(packages_by_id, package_id) for package_id in actual_parent_ids)
        raise PolicyError(
            f"{target}: {child_name} must have exactly one non-dev parent, {expected_parent_name}; "
            f"found {actual or ['<none>']}"
        )


def validate_target_graph(metadata: dict[str, Any], target: str) -> list[str]:
    packages_by_id, ids_by_name = package_maps(metadata)
    adjacency, reverse, features = non_dev_graph(metadata)

    forbidden = sorted(FORBIDDEN_HIGH_LEVEL_TLS_CRATES.intersection(ids_by_name))
    if forbidden:
        raise PolicyError(
            f"{target}: forbidden high-level TLS/OpenSSL crates resolved: {', '.join(forbidden)}"
        )

    core_id = single_package_id(ids_by_name, "mukei-core", target)
    rusqlite_id = single_package_id(ids_by_name, "rusqlite", target)
    sqlite_sys_id = single_package_id(ids_by_name, "libsqlite3-sys", target)
    openssl_sys_id = single_package_id(ids_by_name, "openssl-sys", target)
    openssl_src_id = single_package_id(ids_by_name, "openssl-src", target)

    for package_id, package_label in (
        (rusqlite_id, "rusqlite"),
        (sqlite_sys_id, "libsqlite3-sys"),
        (openssl_sys_id, "openssl-sys"),
        (openssl_src_id, "openssl-src"),
    ):
        if not registry_source_is_crates_io(packages_by_id[package_id]):
            raise PolicyError(f"{target}: {package_label} must resolve from the crates.io registry")

    if "sqlcipher" not in features.get(core_id, set()):
        raise PolicyError(f"{target}: mukei-core/sqlcipher is not active in the all-features release graph")
    if SQLCIPHER_RUSQLITE_FEATURE not in features.get(rusqlite_id, set()):
        raise PolicyError(
            f"{target}: rusqlite feature {SQLCIPHER_RUSQLITE_FEATURE!r} is not active; "
            "the approved SQLCipher vendored-OpenSSL route is missing"
        )

    assert_exact_parent(
        target,
        "openssl-sys",
        openssl_sys_id,
        "libsqlite3-sys",
        sqlite_sys_id,
        reverse,
        packages_by_id,
    )
    assert_exact_parent(
        target,
        "libsqlite3-sys",
        sqlite_sys_id,
        "rusqlite",
        rusqlite_id,
        reverse,
        packages_by_id,
    )
    assert_exact_parent(
        target,
        "openssl-src",
        openssl_src_id,
        "openssl-sys",
        openssl_sys_id,
        reverse,
        packages_by_id,
    )

    if openssl_src_id not in adjacency.get(openssl_sys_id, set()):
        raise PolicyError(
            f"{target}: openssl-sys does not resolve openssl-src; system OpenSSL fallback is not approved"
        )

    can_reach_openssl = ancestors_of(openssl_sys_id, reverse)
    workspace_members = metadata.get("workspace_members", [])
    reviewed_paths: list[str] = []
    found_core_path = False

    for member_id in workspace_members:
        if member_id not in can_reach_openssl:
            continue
        paths = paths_to_target(member_id, openssl_sys_id, adjacency, can_reach_openssl)
        for path in paths:
            names = [package_name(packages_by_id, package_id) for package_id in path]
            if "mukei-core" not in names:
                raise PolicyError(
                    f"{target}: workspace path reaches openssl-sys without mukei-core: {' -> '.join(names)}"
                )
            if tuple(names[-3:]) != APPROVED_OPENSSL_CHAIN_SUFFIX:
                raise PolicyError(
                    f"{target}: unapproved OpenSSL path: {' -> '.join(names)}; expected suffix "
                    f"{' -> '.join(APPROVED_OPENSSL_CHAIN_SUFFIX)}"
                )
            if names[0] == "mukei-core":
                found_core_path = True
            reviewed_paths.append(" -> ".join(names))

    if not found_core_path:
        raise PolicyError(f"{target}: no production path from mukei-core to openssl-sys was found")
    if not reviewed_paths:
        raise PolicyError(f"{target}: openssl-sys resolved but no workspace provenance path was found")

    return sorted(set(reviewed_paths))


def parse_args(argv: Iterable[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--target",
        action="append",
        dest="targets",
        help="Target triple to validate. Repeatable. Defaults to deny.toml [graph].targets.",
    )
    return parser.parse_args(list(argv))


def main(argv: Iterable[str] = ()) -> int:
    args = parse_args(argv)
    try:
        verify_manifest_intent()
        targets = args.targets or shipping_targets()
        for target in targets:
            metadata = cargo_metadata(target)
            paths = validate_target_graph(metadata, target)
            print(f"[openssl-provenance] PASS {target}")
            for path in paths:
                print(f"  {path}")
        print(
            "[openssl-provenance] PASS: openssl-sys is confined to the reviewed "
            "rusqlite -> libsqlite3-sys -> vendored openssl-src SQLCipher path"
        )
        return 0
    except PolicyError as exc:
        print(f"[openssl-provenance] FAIL: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
