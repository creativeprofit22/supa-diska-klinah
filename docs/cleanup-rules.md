# Cleanup rule catalog

Cleanup rules describe bounded discovery and the exact eligibility checks frozen into Rust-owned cleanup plans. A preview is not a backup and may become stale immediately; mutation therefore repeats every applicable rule and filesystem check.

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
| `artifact` | Required for `projectArtifacts` and rejected for `direct`; closed ecosystem, artifact type, confidence, recoverability, and rebuild-consequence metadata. |
| `scanner` | `direct` or `projectArtifacts`. Unknown kinds fail closed. |
| `roots` | Caller binding plus a normalized relative `suffix`; never an environment variable. |
| `markers` | Immediate exact names in `all` and `any`, plus literal `anySuffix`; project scanners require at least one matcher. |
| `targets`, `targetPrefixes`, `targetSuffixes`, `targetType` | Bounded single-component target matchers and `file`, `directory`, or `either`. |
| `rootDepth` | Direct traversal ceiling. Maximum accepted depth is 64. |
| `projectDepth`, `targetDepth` | Required project-discovery and artifact ceilings. |
| `minimumAgeSeconds` | Minimum elapsed age; missing or future timestamps do not satisfy a positive age. |
| `excludedNames`, `excludedPaths` | Case-insensitive components and normalized relative paths never emitted or entered. |

Default limits are 256 rules, 16 roots per rule, 64 values per name field, 64 excluded paths, 512 UTF-8 bytes per text value, and depth 64. Missing marker or target matchers, empty literals, absolute paths, `..`, path separators in names, contradictory scanner fields, unknown artifact enum values, and duplicate IDs are invalid.

Project artifact enums cover the ecosystems, artifact types, confidence levels, and rebuild consequences documented in the [project artifact guide](project-artifacts.md). They serialize as exact camel-case values; extending an enum requires schema fixtures, adapter tests, frontend handling, and documentation.

## Project artifact discovery

The production catalog is embedded from `project-artifact-rules.json`; runtime user catalogs are not loaded. Its marker/target matrix and conservative exclusions are documented in the [project artifact guide](project-artifacts.md). The adapter automatically selects only `verified` and `stable` rules, and every production project rule remains unselected.

Users save at most 32 explicit canonical roots in app-owned storage. All later root operations use opaque IDs. One-root scans use the exact active root; all-root scans omit active children already covered by active parents and divide fixed traversal, candidate, diagnostic, and measurement budgets across the request. Responses have no cleanup scan ID.

## Registered build artifact proofs

Build profiles do not add runtime catalog rules. After deterministic budget selection, Rust constructs one internal exact-component proof for each selected owned generation. The proof uses the generation parent as scan root, the saved project as context root, and binds root, profile, generation, path, type, identity, and measured bytes.

Artifact plan scope defaults to `temporary` for backward compatibility. A `buildArtifact` scope is accepted only when every item carries matching live root and profile IDs plus a valid generation ID. Execution reloads the saved root, profile, registered generation path, role, ownership, and identity; persisted paths and rules never authorize themselves. Only recoverable quarantine is valid for artifact scope.

The normal containment, reparse, activity, identity, exact target, measurement, journal, cross-volume quarantine, undo, and purge checks still run. Generic project markers are not required because native profile approval and the saved exact path provide the context grant. See [build artifact budgets](build-artifact-budgets.md).

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

Preview records receive random opaque IDs. Rust retains the private snapshot and resolves selected candidate IDs into immutable plans; mutation and undo IPC accept only opaque IDs, never paths or rules. Immediately before each item changes, Rust reloads the bounded plan, recompiles current protections, checks strict root and marker-context containment, rejects every reparse component, confirms type and Windows identity, repeats target, exclusion, marker, age, and activity checks, then remeasures logical and allocated bytes. Any mismatch fails that item closed.

## Author checklist

- Bind only the narrowest existing root and use normalized relative suffixes.
- Prefer explicit target names, shallow depths, minimum age, and exclusions.
- Never default-select candidate, deprecated, disabled, or high-impact rules.
- Record original verification evidence; do not copy GPL implementation code.
- Run cleanup-core and Windows adapter tests before proposing promotion.
