# ADR 0001: Modular boundaries

- Status: Accepted
- Date: 2026-08-23

## Context

A Windows cleanup application eventually touches privileged and destructive operating-system surfaces. Its domain contracts must remain testable without Tauri or Windows, while its privileged commands must stay narrowly exposed to the intended local webview. Frontend ownership should follow user features rather than backend layers.

## Decision

Rust dependencies flow only from the thin Tauri application to `windows-platform` to `cleanup-core`. `cleanup-core` owns serializable domain types and contracts. `windows-platform` owns all operating-system interaction. Tauri commands delegate immediately to the adapter.

React route composition lives in `app`. Each feature owns its route object, state, UI, and API adapter. Shared layout cannot import app or feature code, and features cannot import one another.

Tauri uses default-deny access control. Every command must be listed in the app manifest, invoke handler, and a window-scoped capability. Remote origins are not granted. Plugins and protocols are added only for an implemented need.

Automated architecture checks enforce both dependency graphs. The parity matrix records compatibility intent separately from implementation and verification evidence.

## Cleanup safety constraints

Future deletion APIs must be implemented in `windows-platform` and must:

- canonicalize both the allowed root and candidate before changing data;
- prove the candidate remains contained by the allowed root;
- reject symbolic links, junctions, mount points, and other reparse points;
- avoid following links during traversal;
- validate again at the final operation boundary to reduce race exposure;
- make destructive scope explicit and return partial failures without hiding them.

These constraints describe required behavior, not an implementation copied from MangoDisk.

## Rejected alternatives

- **Commands operating directly on Windows:** rejected because UI exposure, platform work, and domain contracts become inseparable.
- **Application depending directly on both crates:** rejected because callers could bypass the adapter boundary.
- **Tauri or Windows types in the core:** rejected because the domain stops being platform-neutral.
- **Global frontend stores organized by IPC module:** rejected because unrelated features become coupled.
- **Broad default capabilities:** rejected because every webview would inherit privileged access.
- **Copying MangoDisk containment code:** rejected because its GPL-3.0-only license is incompatible with the chosen MIT boundary without relicensing the project.

## Consequences

Small features may require changes in several intentionally narrow files. That cost preserves reviewable ownership and limits privileged reach. New cross-feature workflows should be composed at the app boundary rather than through direct feature imports.
