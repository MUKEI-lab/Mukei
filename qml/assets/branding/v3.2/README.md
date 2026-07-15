# Mukei Android branding v3.2

This directory preserves the approved Mukei Android branding handoff without
re-encoding or regenerating the APK production assets.

## APK source of truth

- `mukei_launcher_density_v3_2.tar.xz.b64.part-*` is a lossless Base64 payload
  containing the ten density launcher PNGs used by Android packaging.
- Decoded launcher payload SHA-256:
  `8e9bb74752ae973ed733b9d086d49ecbcb48fb68ae2e59daacccbee15da86557`.
- `FILE_MANIFEST.csv` preserves the original byte count and SHA-256 for every
  item in the complete uploaded handoff.
- `VALIDATION_REPORT.txt`, `CODEX_HANDOFF.md`, and `THEME_SNIPPETS.xml` preserve
  the original integration evidence and instructions.

The Android vector drawables and color resources under `qml/android/res/` are
copied byte-for-byte from the handoff. Density PNGs are materialized from the
pinned payload immediately before an APK build and removed afterwards. This
prevents accidental image recompression while keeping the working tree clean.

The Play Store icon, standalone 1024 px masters, and portrait splash remain
untouched in the original v3.2 handoff. They are intentionally not duplicated
inside the APK source tree because Android does not package them as runtime
resources in this release path.

## Export exact launcher density assets

```bash
python3 scripts/android/prepare-branding.py export \
  --output-dir dist/android/branding-v3.2-launcher
```

The export command verifies the payload SHA-256 and each PNG against
`FILE_MANIFEST.csv` before writing anything.
