# Cleanup rule catalog

Cleanup rules describe **preview-only** discovery. They do not delete, quarantine, restore, or back up files. A preview is not a backup and may become stale immediately.

## Loading and versioning

Catalogs are strict UTF-8 JSON with `schemaVersion: 1`. The loader reads at most 1 MiB and rejects the complete catalog on malformed JSON, unknown fields, unsupported versions, duplicate normalized values, unsafe defaults, traversal, or exceeded limits. Rules use a stable lowercase `id` and a positive `ruleVersion`; change the version whenever behavior changes.

The maintained example is [`catalog-v1.json`](../src-tauri/crates/cleanup-core/tests/fixtures/catalog-v1.json).

## Rule fields

| Field | Meaning |
| --- | --- |
| `id`, `ruleVersion` | Stable identity and positive behavior revision. |
| `lifecycle` | `candidate`, `verified`, `stable`, `deprecated`, or `disabled`. |
| `risk` | `safe`, `recoverable`, or `highImpact`. |
| `provenance` | Nonempty `source` and `verifiedAt` evidence strings. |
| `defaultSelected` | Initial selection; forbidden for candidate, deprecated, disabled, and high-impact rules. |
| `scanner` | `direct` or `projectArtifacts`. Unknown kinds fail closed. |
| `roots` | Caller binding plus a normalized relative `suffix`; never an environment variable. |
| `markers` | Immediate single-component names in `all` and `any`; project scanners require markers. |
| `targets`, `targetType` | Single-component names and `file`, `directory`, or `either`. |
| `rootDepth` | Direct traversal ceiling. Maximum accepted depth is 64. |
| `projectDepth`, `targetDepth` | Required project-discovery and artifact ceilings. |
| `minimumAgeSeconds` | Minimum elapsed age; missing or future timestamps do not satisfy a positive age. |
| `excludedNames`, `excludedPaths` | Case-insensitive components and normalized relative paths never emitted or entered. |

Default limits are 256 rules, 16 roots per rule, 64 values per name field, 64 excluded paths, 512 UTF-8 bytes per text value, and depth 64. Empty targets, absolute paths, `..`, path separators in names, contradictory scanner fields, and duplicate IDs are invalid.

## Lifecycle workflow

1. Add a non-default `candidate` rule with provenance and conservative bounds.
2. Test inaccessible, changing, link-like, protected, and unexpectedly large trees.
3. Promote to `verified` only after Windows fixture and representative-machine review.
4. Promote to `stable` after sustained behavior; increment `ruleVersion` for changes.
5. Use `deprecated` or `disabled` to prevent scanning without reusing its ID.

`highImpact` rules are never selected by default. Risk labels inform selection; they do not bypass protection or validation.

## Scan guarantees

The caller supplies absolute resolved bindings and a compiled protection policy. Scans do not read environment variables or Windows known folders. Traversal is iterative, bounded, cancellable, no-follow, identity checked, and repository metadata aware. Every candidate is checked before and after canonicalization, measured without following links, re-read for changes, then deterministically deduplicated parent-first.

Windows, Program Files, ProgramData, recovery/recycle, profile, documents, cloud-sync, credentials, backup, VM, configured keep paths, and repository metadata must be supplied or recognized as protected. Missing, link-like, identity-less, inaccessible, looping, changing, out-of-root, protected, or truncated candidates are diagnosed and never previewed.

Preview records receive random opaque IDs. Paths remain privately resolvable by the snapshot for a future platform operation. This phase exposes no deletion or IPC command. Any future mutation must revalidate identity, containment, protections, and reparse state immediately before changing data; preview checks cannot eliminate that race.

## Author checklist

- Bind only the narrowest existing root and use normalized relative suffixes.
- Prefer explicit target names, shallow depths, minimum age, and exclusions.
- Never default-select candidate, deprecated, disabled, or high-impact rules.
- Record original verification evidence; do not copy GPL implementation code.
- Run cleanup-core and Windows adapter tests before proposing promotion.
