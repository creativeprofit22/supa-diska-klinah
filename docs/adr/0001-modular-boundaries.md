# ADR 0001: Modular boundaries

- Status: Accepted
- Date: 2026-08-23

## Context

A Windows cleanup application eventually touches privileged and destructive operating-system surfaces. Its domain contracts must remain testable without Tauri or Windows, while its privileged commands must stay narrowly exposed to the intended local webview. Frontend ownership should follow user features rather than backend layers.

## Decision

Rust dependencies flow from the thin Tauri application and separately packaged `privileged-helper` into `windows-platform`, then into `cleanup-core`. `cleanup-core` owns serializable domain types and contracts. `windows-platform` owns operating-system interaction, privilege policy, helper protocol, and path containment. The helper contains only its entry point and fixed exit mapping; neither helper nor domain crates depend on Tauri.

React route composition lives in `app`. Each feature owns its route object, state, UI, and API adapter. Shared layout cannot import app or feature code, and features cannot import one another.

Tauri uses default-deny access control. Every command must be listed in the app manifest, invoke handler, and a local main-webview capability. Navigation accepts only the packaged origin and one exact development origin. Remote origins and generic shell, filesystem, process, and sidecar permissions are not granted.

The main executable is `asInvoker` and rejects elevated startup. Genuinely administrative work uses one separately manifested `requireAdministrator` helper. Its one-shot authenticated loopback protocol exposes an operation enum, never arbitrary commands or paths. Restore-point creation is the only approved privileged operation.

Automated architecture and security checks enforce dependency graphs, synchronized command allowlists, manifests, and capability policy. The parity matrix records compatibility intent separately from implementation and verification evidence.

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
- **Relaunching the whole app as administrator:** rejected because webview and ordinary operations would inherit high integrity.
- **Generic sidecar, shell, or filesystem IPC:** rejected because frontend-controlled arguments would reach privileged sinks.
- **Long-lived privileged service:** rejected because one restore operation does not justify persistent authority or credentials.
- **Copying MangoDisk containment code:** rejected because its GPL-3.0-only license is incompatible with the chosen MIT boundary without relicensing the project.

## Consequences

Small features may require changes in several intentionally narrow files. Commands must stay synchronized across three allowlists, and every new privileged enum variant requires threat-model review. That cost preserves reviewable ownership and limits privileged reach. New cross-feature workflows should be composed at the app boundary rather than through direct feature imports.
