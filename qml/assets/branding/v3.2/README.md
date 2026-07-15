# Mukei Android branding v3.2

This directory preserves the approved Mukei Android branding handoff without
re-encoding or regenerating any production PNG.

## Source of truth

- `mukei_branding_v3_2_payload.tar.xz.b64.part-*` is a lossless Base64 payload
  containing every production PNG from `01_STANDALONE_PNGS/` in the approved
  handoff. Duplicate overlay PNGs and the reference-only contact sheet are not
  stored twice.
- Decoded payload SHA-256:
  `59ea54b2313bff03cc08022cff741b5b5584ec731a59b05a4e68ad762a0d1e84`.
- `FILE_MANIFEST.csv` contains the original byte count and SHA-256 for every
  handoff deliverable.
- `VALIDATION_REPORT.txt` is the package's original validation report.

The Android vector drawables and color resources under `qml/android/res/` are
copied byte-for-byte from the handoff. Density PNGs are materialized from the
pinned payload immediately before an APK build and removed afterwards. This
prevents accidental image recompression while keeping the working tree clean.

## Export the standalone production assets

```bash
python3 scripts/android/prepare-branding.py export \
  --output-dir dist/android/branding-v3.2
```

The export command verifies the payload hash and every exported file against
`FILE_MANIFEST.csv` before writing anything.
