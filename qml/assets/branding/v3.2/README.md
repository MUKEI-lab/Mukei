# Mukei Android branding v3.2

This directory preserves the approved Mukei Android branding handoff without
re-encoding or regenerating any production PNG.

## Source of truth

- `mukei_android_branding_v3_2_codex_handoff.zip.b64` is the original uploaded
  ZIP encoded as Base64 so it can be committed through text-only automation.
- Decoded ZIP SHA-256:
  `4d069a8ff36bc3057c0d021b0f6b9a02649e96e36299ffc9ab63b5e039ea781d`.
- `FILE_MANIFEST.csv` contains the byte count and SHA-256 for every handoff
  deliverable.
- `VALIDATION_REPORT.txt` is the package's original validation report.

The Android vector drawables and color resources under `qml/android/res/` are
copied byte-for-byte from the handoff. Density PNGs are materialized from the
pinned archive immediately before an APK build and removed afterwards. This
prevents accidental image recompression while keeping the working tree clean.

## Export the standalone production assets

```bash
python3 scripts/android/prepare-branding.py export \
  --output-dir dist/android/branding-v3.2
```

The export command verifies the ZIP hash and every file listed in
`FILE_MANIFEST.csv` before writing anything.
