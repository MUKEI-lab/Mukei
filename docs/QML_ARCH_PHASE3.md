# QML Architecture Phase 3 Implementation

## Feature projections

Phase 3 introduces independent feature stores for:

- Models
- Durable downloads
- Storage pressure
- Private documents
- Settings
- Responsive layout

Each store follows the architecture contract: hydrate a backend snapshot, apply typed deltas where available, expose reactive state, and dispatch all mutations through `IntentDispatcher`.

## Privacy boundary

The download projection returns opaque destination tokens only. The private-document projection uses a stable hash-derived document identifier and never exposes raw SAF tokens or content URIs. Errors remain routed through the central redaction-aware error projection.

## Settings

`SettingsStore` hydrates the non-secret `preferences` repository and persists only allow-listed typed settings. Provider credentials remain outside this path. Theme, motion, contrast, font scaling, inference defaults, and remote policy are validated by Rust before persistence.

## Responsive shell

`ResponsiveStore` derives compact, medium, and expanded modes from viewport width. Medium and expanded modes use an adaptive side navigation surface. Feature state and domain state do not change between layouts.

## Accessibility primitives

Primary, secondary, ghost, destructive, and icon controls now derive from Qt Quick Controls `Button`. They provide keyboard activation, focus semantics, accessible roles/names, and minimum touch targets while preserving Mukei's gentle motion system.

## Remaining contracts

Model switching, safe model deletion, document attach/revoke, diagnostics export, and expanded split-view chat require explicit backend commands before their UI actions are exposed. The frontend intentionally avoids simulated success for those operations.
