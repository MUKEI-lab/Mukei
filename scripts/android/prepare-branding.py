#!/usr/bin/env python3
"""Verify and materialize the approved Mukei Android branding v3.2 assets."""

from __future__ import annotations

import argparse
import base64
import csv
import hashlib
import io
import json
import sys
import tarfile
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[2]
BRAND_ROOT = ROOT / "qml" / "assets" / "branding" / "v3.2"
PAYLOAD_PARTS = "mukei_branding_v3_2_payload.tar.xz.b64.part-*"
EXPECTED_PAYLOAD_SHA256 = "59ea54b2313bff03cc08022cff741b5b5584ec731a59b05a4e68ad762a0d1e84"
MANIFEST_PATH = BRAND_ROOT / "FILE_MANIFEST.csv"
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"

DENSITY_MAP = {
    "01_STANDALONE_PNGS/android_density_pngs/ic_launcher_mdpi_48x48.png": "mipmap-mdpi/ic_launcher.png",
    "01_STANDALONE_PNGS/android_density_pngs/ic_launcher_round_mdpi_48x48.png": "mipmap-mdpi/ic_launcher_round.png",
    "01_STANDALONE_PNGS/android_density_pngs/ic_launcher_hdpi_72x72.png": "mipmap-hdpi/ic_launcher.png",
    "01_STANDALONE_PNGS/android_density_pngs/ic_launcher_round_hdpi_72x72.png": "mipmap-hdpi/ic_launcher_round.png",
    "01_STANDALONE_PNGS/android_density_pngs/ic_launcher_xhdpi_96x96.png": "mipmap-xhdpi/ic_launcher.png",
    "01_STANDALONE_PNGS/android_density_pngs/ic_launcher_round_xhdpi_96x96.png": "mipmap-xhdpi/ic_launcher_round.png",
    "01_STANDALONE_PNGS/android_density_pngs/ic_launcher_xxhdpi_144x144.png": "mipmap-xxhdpi/ic_launcher.png",
    "01_STANDALONE_PNGS/android_density_pngs/ic_launcher_round_xxhdpi_144x144.png": "mipmap-xxhdpi/ic_launcher_round.png",
    "01_STANDALONE_PNGS/android_density_pngs/ic_launcher_xxxhdpi_192x192.png": "mipmap-xxxhdpi/ic_launcher.png",
    "01_STANDALONE_PNGS/android_density_pngs/ic_launcher_round_xxxhdpi_192x192.png": "mipmap-xxxhdpi/ic_launcher_round.png",
}


def fail(message: str) -> "NoReturn":
    raise SystemExit(f"branding preparation failed: {message}")


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def load_manifest() -> dict[str, tuple[int, str]]:
    if not MANIFEST_PATH.is_file():
        fail(f"branding manifest is missing: {MANIFEST_PATH.relative_to(ROOT)}")
    manifest: dict[str, tuple[int, str]] = {}
    with MANIFEST_PATH.open(encoding="utf-8-sig", newline="") as handle:
        for row in csv.DictReader(handle):
            path = row.get("file", "").strip()
            if not path:
                fail("FILE_MANIFEST.csv contains an empty path")
            try:
                expected_size = int(row["bytes"])
                expected_hash = row["sha256"].strip().lower()
            except (KeyError, TypeError, ValueError) as error:
                fail(f"invalid manifest row for {path}: {error}")
            manifest[path] = (expected_size, expected_hash)
    return manifest


def load_payload() -> tuple[dict[str, bytes], dict[str, tuple[int, str]]]:
    parts = sorted(BRAND_ROOT.glob(PAYLOAD_PARTS))
    if not parts:
        fail(f"branding payload parts are missing: {BRAND_ROOT.relative_to(ROOT)}/{PAYLOAD_PARTS}")
    try:
        encoded = "".join(part.read_text(encoding="ascii") for part in parts)
        encoded = "".join(encoded.split())
        payload = base64.b64decode(encoded, validate=True)
    except (OSError, ValueError) as error:
        fail(f"branding payload is not valid Base64: {error}")
    digest = sha256(payload)
    if digest != EXPECTED_PAYLOAD_SHA256:
        fail(f"branding payload SHA-256 mismatch: expected {EXPECTED_PAYLOAD_SHA256}, got {digest}")

    manifest = load_manifest()
    files: dict[str, bytes] = {}
    try:
        with tarfile.open(fileobj=io.BytesIO(payload), mode="r:xz") as archive:
            for member in archive.getmembers():
                if not member.isfile():
                    continue
                pure = PurePosixPath(member.name)
                if pure.is_absolute() or ".." in pure.parts:
                    fail(f"unsafe path in branding payload: {member.name}")
                handle = archive.extractfile(member)
                if handle is None:
                    fail(f"could not read branding payload member: {member.name}")
                data = handle.read()
                if member.name == "FILE_MANIFEST.csv":
                    continue
                files[member.name] = data
    except (tarfile.TarError, EOFError, OSError) as error:
        fail(f"branding payload is invalid: {error}")

    expected_paths = {path for path in manifest if path.startswith("01_STANDALONE_PNGS/")}
    if set(files) != expected_paths:
        missing = sorted(expected_paths - set(files))
        extra = sorted(set(files) - expected_paths)
        fail(f"branding payload membership mismatch; missing={missing}, extra={extra}")
    for path, data in files.items():
        expected_size, expected_hash = manifest[path]
        if len(data) != expected_size:
            fail(f"byte count mismatch for {path}")
        if sha256(data) != expected_hash:
            fail(f"SHA-256 mismatch for {path}")
    return files, manifest


def png_dimensions(data: bytes) -> tuple[int, int]:
    if len(data) < 24 or not data.startswith(PNG_SIGNATURE) or data[12:16] != b"IHDR":
        fail("invalid PNG data in branding payload")
    return int.from_bytes(data[16:20], "big"), int.from_bytes(data[20:24], "big")


def verify() -> None:
    files, _ = load_payload()
    expected_sizes = {
        "mipmap-mdpi/ic_launcher.png": (48, 48),
        "mipmap-mdpi/ic_launcher_round.png": (48, 48),
        "mipmap-hdpi/ic_launcher.png": (72, 72),
        "mipmap-hdpi/ic_launcher_round.png": (72, 72),
        "mipmap-xhdpi/ic_launcher.png": (96, 96),
        "mipmap-xhdpi/ic_launcher_round.png": (96, 96),
        "mipmap-xxhdpi/ic_launcher.png": (144, 144),
        "mipmap-xxhdpi/ic_launcher_round.png": (144, 144),
        "mipmap-xxxhdpi/ic_launcher.png": (192, 192),
        "mipmap-xxxhdpi/ic_launcher_round.png": (192, 192),
    }
    for source, destination in DENSITY_MAP.items():
        actual = png_dimensions(files[source])
        expected = expected_sizes[destination]
        if actual != expected:
            fail(f"unexpected dimensions for {source}: {actual}, expected {expected}")
    print("Mukei branding v3.2 payload verified")
    print(f"  payload SHA-256: {EXPECTED_PAYLOAD_SHA256}")
    print(f"  production PNGs: {len(files)}")
    print("  launcher PNG densities: complete")


def materialize(repo_root: Path, state_path: Path) -> None:
    files, manifest = load_payload()
    created: list[dict[str, str]] = []
    destination_root = repo_root / "qml" / "android" / "res"
    for source, relative in DENSITY_MAP.items():
        expected_hash = manifest[source][1]
        destination = destination_root / Path(relative)
        data = files[source]
        destination.parent.mkdir(parents=True, exist_ok=True)
        if destination.exists():
            if sha256(destination.read_bytes()) != expected_hash:
                fail(f"refusing to overwrite non-matching brand asset: {destination}")
            continue
        destination.write_bytes(data)
        created.append({"path": str(destination.relative_to(repo_root)), "sha256": expected_hash})

    state_path.parent.mkdir(parents=True, exist_ok=True)
    state_path.write_text(json.dumps({"created": created}, indent=2) + "\n", encoding="utf-8")
    print(f"Materialized {len(created)} exact branding PNGs")


def cleanup(repo_root: Path, state_path: Path) -> None:
    if not state_path.is_file():
        return
    try:
        state = json.loads(state_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"invalid branding state file: {error}")
    for item in reversed(state.get("created", [])):
        path = repo_root / item["path"]
        if not path.exists():
            continue
        if sha256(path.read_bytes()) != item["sha256"]:
            fail(f"refusing to remove modified generated asset: {path}")
        path.unlink()
        parent = path.parent
        while parent != repo_root and parent.exists():
            try:
                parent.rmdir()
            except OSError:
                break
            parent = parent.parent
    state_path.unlink(missing_ok=True)


def export_assets(output_dir: Path) -> None:
    files, _ = load_payload()
    if output_dir.exists() and any(output_dir.iterdir()):
        fail(f"export directory is not empty: {output_dir}")
    output_dir.mkdir(parents=True, exist_ok=True)
    prefix = PurePosixPath("01_STANDALONE_PNGS")
    for source, data in files.items():
        relative = PurePosixPath(source).relative_to(prefix)
        destination = output_dir.joinpath(*relative.parts)
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(data)
    print(f"Exported {len(files)} verified branding PNGs to {output_dir}")


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser()
    sub = root.add_subparsers(dest="command", required=True)
    sub.add_parser("verify")
    materialize_parser = sub.add_parser("materialize")
    materialize_parser.add_argument("--repo-root", type=Path, default=ROOT)
    materialize_parser.add_argument("--state", type=Path, required=True)
    cleanup_parser = sub.add_parser("cleanup")
    cleanup_parser.add_argument("--repo-root", type=Path, default=ROOT)
    cleanup_parser.add_argument("--state", type=Path, required=True)
    export_parser = sub.add_parser("export")
    export_parser.add_argument("--output-dir", type=Path, required=True)
    return root


def main() -> int:
    args = parser().parse_args()
    if args.command == "verify":
        verify()
    elif args.command == "materialize":
        materialize(args.repo_root.resolve(), args.state.resolve())
    elif args.command == "cleanup":
        cleanup(args.repo_root.resolve(), args.state.resolve())
    elif args.command == "export":
        export_assets(args.output_dir.resolve())
    return 0


if __name__ == "__main__":
    sys.exit(main())
