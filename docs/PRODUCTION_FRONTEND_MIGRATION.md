# Production frontend migration

Working branch: `agent/full-qml-fixed-build-2026-07-15`

This migration promotes the proven Android/QML runtime fixes into canonical source and aligns the mobile shell, chat, settings, typography, spacing, privacy surfaces, and responsive behavior with `rust/docs/UXB.md` v2.1.

The migration is not complete until the real CXX-Qt bridge APK builds, signs, validates, and survives physical-device smoke testing.
