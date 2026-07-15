#!/usr/bin/env python3
"""Verify and materialize approved Mukei Android launcher PNGs without recompression."""
from __future__ import annotations

import argparse, base64, csv, hashlib, io, json, sys, tarfile
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[2]
BRAND_ROOT = ROOT / "qml/assets/branding/v3.2"
PAYLOAD_GLOB = "mukei_launcher_density_v3_2.tar.xz.b64.part-*"
PAYLOAD_SHA256 = "8e9bb74752ae973ed733b9d086d49ecbcb48fb68ae2e59daacccbee15da86557"
MANIFEST = BRAND_ROOT / "FILE_MANIFEST.csv"
PNG = b"\x89PNG\r\n\x1a\n"
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
SIZES = {"mdpi": 48, "hdpi": 72, "xhdpi": 96, "xxhdpi": 144, "xxxhdpi": 192}


def fail(message: str) -> "NoReturn":
    raise SystemExit(f"branding preparation failed: {message}")


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def manifest() -> dict[str, tuple[int, str]]:
    rows: dict[str, tuple[int, str]] = {}
    try:
        with MANIFEST.open(encoding="utf-8-sig", newline="") as stream:
            for row in csv.DictReader(stream):
                rows[row["file"]] = (int(row["bytes"]), row["sha256"].lower())
    except (OSError, KeyError, ValueError) as error:
        fail(f"invalid branding manifest: {error}")
    return rows


def load_payload() -> tuple[dict[str, bytes], dict[str, tuple[int, str]]]:
    parts = sorted(BRAND_ROOT.glob(PAYLOAD_GLOB))
    if not parts:
        fail(f"missing payload parts matching {PAYLOAD_GLOB}")
    try:
        encoded = "".join("".join(path.read_text(encoding="ascii").split()) for path in parts)
        payload = base64.b64decode(encoded, validate=True)
    except (OSError, ValueError) as error:
        fail(f"invalid Base64 payload: {error}")
    if digest(payload) != PAYLOAD_SHA256:
        fail("launcher payload SHA-256 mismatch")

    expected = manifest()
    files: dict[str, bytes] = {}
    try:
        with tarfile.open(fileobj=io.BytesIO(payload), mode="r:xz") as archive:
            for member in archive.getmembers():
                pure = PurePosixPath(member.name)
                if not member.isfile() or pure.is_absolute() or ".." in pure.parts:
                    if member.isfile():
                        fail(f"unsafe payload member: {member.name}")
                    continue
                stream = archive.extractfile(member)
                if stream is None:
                    fail(f"unreadable payload member: {member.name}")
                files[member.name] = stream.read()
    except (tarfile.TarError, OSError, EOFError) as error:
        fail(f"invalid launcher payload: {error}")

    if set(files) != set(DENSITY_MAP):
        fail("launcher payload membership mismatch")
    for name, data in files.items():
        size, sha = expected[name]
        if len(data) != size or digest(data) != sha:
            fail(f"manifest mismatch for {name}")
    return files, expected


def dimensions(data: bytes) -> tuple[int, int]:
    if len(data) < 24 or not data.startswith(PNG) or data[12:16] != b"IHDR":
        fail("payload contains invalid PNG data")
    return int.from_bytes(data[16:20], "big"), int.from_bytes(data[20:24], "big")


def verify() -> None:
    files, _ = load_payload()
    for source, target in DENSITY_MAP.items():
        density = target.split("/", 1)[0].removeprefix("mipmap-")
        expected = (SIZES[density], SIZES[density])
        if dimensions(files[source]) != expected:
            fail(f"dimension mismatch for {source}")
    print("Mukei branding v3.2 launcher payload verified")
    print(f"  payload SHA-256: {PAYLOAD_SHA256}")
    print(f"  exact density PNGs: {len(files)}")


def materialize(repo: Path, state: Path) -> None:
    files, expected = load_payload()
    created = []
    root = repo / "qml/android/res"
    for source, relative in DENSITY_MAP.items():
        destination = root / relative
        wanted = expected[source][1]
        destination.parent.mkdir(parents=True, exist_ok=True)
        if destination.exists():
            if digest(destination.read_bytes()) != wanted:
                fail(f"refusing to overwrite modified asset: {destination}")
            continue
        destination.write_bytes(files[source])
        created.append({"path": str(destination.relative_to(repo)), "sha256": wanted})
    state.parent.mkdir(parents=True, exist_ok=True)
    state.write_text(json.dumps({"created": created}, indent=2) + "\n", encoding="utf-8")
    print(f"Materialized {len(created)} exact launcher PNGs")


def cleanup(repo: Path, state: Path) -> None:
    if not state.is_file():
        return
    record = json.loads(state.read_text(encoding="utf-8"))
    for item in reversed(record.get("created", [])):
        path = repo / item["path"]
        if path.exists():
            if digest(path.read_bytes()) != item["sha256"]:
                fail(f"refusing to remove modified asset: {path}")
            path.unlink()
    state.unlink(missing_ok=True)


def export(output: Path) -> None:
    files, _ = load_payload()
    if output.exists() and any(output.iterdir()):
        fail(f"export directory is not empty: {output}")
    output.mkdir(parents=True, exist_ok=True)
    prefix = PurePosixPath("01_STANDALONE_PNGS/android_density_pngs")
    for source, data in files.items():
        destination = output.joinpath(*PurePosixPath(source).relative_to(prefix).parts)
        destination.write_bytes(data)
    print(f"Exported {len(files)} exact launcher PNGs to {output}")


def main() -> int:
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("verify")
    for name in ("materialize", "cleanup"):
        command = sub.add_parser(name)
        command.add_argument("--repo-root", type=Path, default=ROOT)
        command.add_argument("--state", type=Path, required=True)
    command = sub.add_parser("export")
    command.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "verify": verify()
    elif args.command == "materialize": materialize(args.repo_root.resolve(), args.state.resolve())
    elif args.command == "cleanup": cleanup(args.repo_root.resolve(), args.state.resolve())
    else: export(args.output_dir.resolve())
    return 0


if __name__ == "__main__":
    sys.exit(main())
