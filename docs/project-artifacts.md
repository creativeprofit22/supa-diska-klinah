# Project artifact discovery

Project artifact discovery finds rebuildable storage beneath roots you explicitly save. It does not search a drive automatically, select results, create cleanup plans, or delete project files. Every result is **Not selected**.

## Managing roots

- Add one existing absolute directory path. Rust trims surrounding whitespace, canonicalizes the directory, and rejects files, drive roots, protected locations, lexical parent traversal, missing identities, and reparse points.
- Up to 32 roots are stored in the app-owned `cleanup/project-roots.json` file beneath app data using flush-and-rename persistence.
- After add, list, pause, resume, remove, and scan operations use an opaque root ID. Paths are not accepted again through those commands.
- **Scan** rescans one active saved root. **Scan active roots** scans every active root. Opening Cleanup only lists roots; it does not scan.
- Pausing preserves a root but omits it from all-root scans. Resuming makes it eligible again.
- Removing a root only forgets the registry entry. It never changes files beneath that root.
- A successful scan updates that root's last-scan time. A failed or collapsed child root is not reported as scanned.

For an all-root scan, canonical roots are ordered parent-first. An active child already contained by an active parent is omitted from that request. Scanning the child by its own ID still scans that exact root. A paused parent does not suppress an active child.

## Supported ecosystems

Names are matched case-insensitively as single path components. Exact markers must be present in the immediate project directory. Prefix and suffix matchers require a nonempty additional literal. Targets are generated directories documented by their ecosystems.

| Ecosystem | Required project marker | Generated targets | Rebuild consequence |
| --- | --- | --- | --- |
| Rust | `Cargo.toml` | `target` | Toolchain required |
| Node.js | `package.json` | `node_modules` | Network download required |
| Next.js | `package.json` and a known `next.config` file | `.next`, `out` | Local rebuild |
| Angular | `package.json` and `angular.json` | `.angular` | Local rebuild |
| Nuxt | `package.json` and a known `nuxt.config` file | `.nuxt`, `.output` | Local rebuild |
| Vite | `package.json` and a known `vite.config` file | `.vite`, `dist` | Local rebuild |
| SvelteKit | `package.json` and a known `svelte.config` file | `.svelte-kit`, `build` | Local rebuild |
| Astro | `package.json` and a known `astro.config` file | `.astro`, `dist` | Local rebuild |
| Python environments | One of `pyproject.toml`, `setup.py`, `setup.cfg`, `requirements.txt`, `Pipfile` | `.venv`, `venv` | Network download required |
| Python caches | Same Python markers | `__pycache__`, `.pytest_cache`, `.mypy_cache`, `.ruff_cache`, `.tox`, `.nox` | Local rebuild |
| Python build output | Same Python markers | `build`, `dist`, names ending `.egg-info` | Local rebuild |
| .NET | A name ending `.csproj`, `.fsproj`, `.vbproj`, `.sln`, or `.slnx` | `bin`, `obj` | Toolchain required |
| Gradle | A Gradle build or settings file | `.gradle`, `build`, `outputs` | Local rebuild |
| Maven | `pom.xml` | `target` | Local rebuild |
| CMake | `CMakeLists.txt` | `build`, `out`, names beginning `cmake-build-` | Toolchain required |
| Unity | `Assets` and `ProjectSettings` | `Library`, `Temp`, `obj` | Expensive asset reimport |
| Unreal Engine | A name ending `.uproject` | `Binaries`, `Intermediate`, `DerivedDataCache` | Toolchain rebuild or expensive asset reimport |
| Godot | `project.godot` | `.godot` | Expensive asset reimport |

A generic `target`, `build`, `out`, `Library`, or `outputs` directory is not enough. The compatible marker context must exist first. This is the primary collision control.

## Reading results

Results are grouped by project path and ecosystem.

- **Artifact type** distinguishes dependencies, build output, compiler or framework caches, virtual environments, test caches, generated intermediates, and imported asset caches.
- **Size** is bounded logical content measured without following links. Concurrent disk changes can make it approximate immediately.
- **Age** is calculated once from the scan clock and the latest measured modification time. Missing timestamps show unavailable; future timestamps show zero rather than underflowing.
- **Modified** is the exact available timestamp used for display.
- **Activity** is `Idle` when the adapter can obtain the required read-only availability probe and `In use` otherwise. In-use project artifacts remain visible but unselected. Existing cleanup revalidation still rejects active items.
- **Confidence** is `High` or `Medium` from the reviewed marker set in the rule. It is not guessed by the frontend.
- **Risk** describes collision and recovery impact. It never bypasses validation.
- **Recoverability** is currently limited to `Rebuildable`.
- **Rebuild consequence** states whether recovery is local, needs downloads, needs a toolchain, or can trigger an expensive asset reimport.
- **Not selected** is unconditional for production project rules. Project discovery exposes no checkbox, select-all, cleanup, or delete action.

## Monorepos and nested repositories

Traversal continues below the selected code root and may recognize nested marker-bearing project directories. The scanner does not enter `.git`, but it can inspect a nested repository directory itself. Sibling packages retain separate artifacts.

Candidates retain both project-context and artifact identities. Before a result is accepted, Rust repeats marker matching and verifies no-follow metadata, canonical containment in both the selected root and marker context, identity, type, target matcher, exclusions, and bounded measurement.

Exact duplicate artifact paths or identities are emitted once. Parent-child artifact overlap is resolved parent-first, and a bounded diagnostic reports the suppressed child. Matched artifact trees are measured but not searched as additional project contexts. This prevents nested caches from being counted twice while preserving unrelated sibling outputs.

## Conservative exclusions and expected false negatives

The catalog intentionally omits locations that can contain user work, diagnostics, autosaves, or configured deploy output:

- Unreal `Build` and `Saved`
- Unity `Logs`
- generic `cache` and arbitrary `output`
- logs, source files, checked-in assets, and autosaves
- framework-configured custom output directories

Rules do not parse repositories, manifests, lock files, or framework configuration. They do not invoke shells, package managers, compilers, or network requests. A custom output path or unusual config filename can therefore be missed. Add the documented standard target or wait for a reviewed rule rather than broadening a generic matcher.

## Offline and in-use considerations

`Rebuildable` does not mean cheap or available offline. Dependency folders and virtual environments can require network access. Rust, .NET, CMake, and Unreal outputs can require installed toolchains. Unity, Unreal, and Godot caches can trigger lengthy imports. Review the displayed consequence before manually changing anything outside this feature.

An in-use result can belong to an editor, build, test, package-manager, or game-engine process. Close the owning tool and rescan if you need current size or activity. Discovery itself remains read-only.

## Troubleshooting

| Symptom | What to check |
| --- | --- |
| Root is invalid | Use an existing absolute directory below, not equal to, a drive root. Remove `..` components. |
| Root is protected | Choose a code workspace outside Windows, recovery, profile-document, cloud-sync, credential, backup, VM, repository-metadata, and configured keep locations. |
| Root is a link or junction | Save the real canonical directory. Symlinks, junctions, mount-like reparse points, and changed identities fail closed. |
| Permission or scan failure | Confirm the standard user can read the root, then rescan. Errors remain fixed and do not expose filesystem details. |
| A scan stops early | Narrow the saved root. Root count, visited entries, candidate drafts, diagnostics, measurement entries, workers, and output records have aggregate caps. |
| Results look stale | Check the root's last-scan time and run an explicit rescan. Files can change after any scan. |
| A nested root was not scanned | An active parent suppresses active children during all-root scans. Use the child's Scan button, or pause the parent. |
| Custom output is missing | Custom framework and deploy paths are deliberately not inferred. Only reviewed documented targets are recognized. |

## Authoring and promoting rules

Production rules live in `src-tauri/crates/windows-platform/src/cleanup/project-artifact-rules.json`. Runtime user catalogs are not loaded, so webview input cannot approve a new rule or alter scanner behavior.

A project rule defines:

- stable `id`, positive `ruleVersion`, lifecycle, risk, provenance, and `defaultSelected: false`;
- closed artifact ecosystem, type, confidence, recoverability, and rebuild consequence;
- `scanner: projectArtifacts` and an app-resolved root binding;
- at least one exact marker in `markers.all` or `markers.any`, or a literal suffix in `markers.anySuffix`;
- at least one exact `targets` name, `targetPrefixes` literal, or `targetSuffixes` literal;
- target type plus bounded project and target depths;
- exact excluded names and normalized relative excluded paths where needed.

Matchers are not globs or regular expressions. Every matcher is one bounded filename component with no separator, control character, `.` or `..`. Prefix and suffix values are literals, and matches must include additional text. The same helper performs initial discovery and final revalidation.

Use this promotion workflow:

1. Cite current official ecosystem documentation in provenance.
2. Add the rule as `candidate` and keep it unselected.
3. Add representative fixtures, generic-name collision tests, malformed matcher tests, containment and link tests, and an expected false-negative note.
4. Review behavior and licensing. Behavioral comparisons to GPL software must not copy tables, functions, identifiers, comments, tests, or control flow.
5. Promote to `verified` only after provenance and fixtures pass review.
6. Promote to `stable` after sustained use. Increment `ruleVersion` when behavior changes.
7. Use `deprecated` or `disabled` to stop automatic scanning without reusing the ID.

The production adapter selects only `verified` and `stable` rules. Candidate, deprecated, and disabled rules remain absent even when present in the embedded catalog.

## Safety boundary

Project scanning remains read-only. Its private cleanup-core snapshot is discarded immediately. Responses contain roots, display records, bounded diagnostics, and scan time, but no reusable cleanup scan ID. Discovery paths and records never feed cleanup-plan creation.

The separate opt-in [build artifact budget](build-artifact-budgets.md) feature starts only after a user registers exact relative paths under a saved root and approves the executable plus immutable argv in a native Windows prompt. Discovery does not prefill, authorize, own, or select those paths. Dependency and incremental roles remain protected, and automatic `cargo clean` is forbidden.

See also [cleanup rule authoring](cleanup-rules.md), [architecture](architecture.md), [security](security.md), and [parity](parity.md).
