import { useMemo, useState, type FormEvent } from "react";
import {
  type ArtifactEcosystem,
  type ArtifactType,
  type ProjectArtifactRecord,
} from "./api/previewCleanup";
import { formatBytes, formatModified } from "./format";
import { useProjectArtifactDiscovery } from "./model/useProjectArtifactDiscovery";

const MAX_PROJECT_ROOT_BYTES = 4_096;
const PROJECT_ROOT_LENGTH_ERROR = "Project root must be 4,096 UTF-8 bytes or fewer.";
const ECOSYSTEM_LABELS: Record<ArtifactEcosystem, string> = {
  rust: "Rust",
  nodeJs: "Node.js",
  nextJs: "Next.js",
  angular: "Angular",
  nuxt: "Nuxt",
  vite: "Vite",
  svelteKit: "SvelteKit",
  astro: "Astro",
  python: "Python",
  dotNet: ".NET",
  gradle: "Gradle",
  maven: "Maven",
  cmake: "CMake",
  unity: "Unity",
  unreal: "Unreal Engine",
  godot: "Godot",
};
const ARTIFACT_LABELS: Record<ArtifactType, string> = {
  installedDependencies: "Installed dependencies",
  buildOutput: "Build output",
  compilerCache: "Compiler cache",
  frameworkCache: "Framework cache",
  virtualEnvironment: "Virtual environment",
  testCache: "Test cache",
  generatedIntermediate: "Generated intermediate",
  importedAssetCache: "Imported asset cache",
};
const CONSEQUENCE_LABELS = {
  localRebuild: "Local rebuild",
  networkDownloadRequired: "Network download required",
  toolchainRequired: "Toolchain required",
  expensiveReimport: "Expensive asset reimport",
} as const;

function formatAge(seconds?: number | null): string {
  if (seconds == null) return "Unavailable";
  if (seconds < 60) return `${seconds} seconds`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} ${minutes === 1 ? "minute" : "minutes"}`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} ${hours === 1 ? "hour" : "hours"}`;
  const days = Math.floor(hours / 24);
  return `${days} ${days === 1 ? "day" : "days"}`;
}

interface ProjectGroup {
  key: string;
  projectName: string;
  projectPath: string;
  ecosystem: ArtifactEcosystem;
  records: ProjectArtifactRecord[];
}

export function ProjectArtifactDiscovery() {
  const [root, setRoot] = useState("");
  const state = useProjectArtifactDiscovery();
  const rootTooLong = new TextEncoder().encode(root).length > MAX_PROJECT_ROOT_BYTES;
  const busy = state.loadingRoots || state.scanning || state.pending !== null;
  const activeRoots = state.roots.filter((saved) => !saved.paused);
  const groups = useMemo(() => {
    const grouped = new Map<string, ProjectGroup>();
    for (const record of state.result?.records ?? []) {
      const key = `${record.projectPath}\u0000${record.artifact.ecosystem}`;
      const group = grouped.get(key) ?? {
        key,
        projectName: record.projectName,
        projectPath: record.projectPath,
        ecosystem: record.artifact.ecosystem,
        records: [],
      };
      group.records.push(record);
      grouped.set(key, group);
    }
    return [...grouped.values()];
  }, [state.result]);

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (busy || rootTooLong || !root.trim()) return;
    void state.add(root).then((added) => {
      if (added) setRoot("");
    });
  }

  return (
    <section className="project-artifacts" aria-labelledby="project-artifacts-heading">
      <div className="project-artifacts-header">
        <div>
          <p className="kicker">Developer storage</p>
          <h2 id="project-artifacts-heading">Coding project artifacts</h2>
        </div>
        <p id="project-root-help">
          Save explicit project roots, then scan marker-backed rebuildable files. Nothing is selected
          or removed.
        </p>
      </div>

      <form className="project-artifact-form" onSubmit={submit}>
        <label htmlFor="project-root">Add an absolute project path</label>
        <div className="project-artifact-form-row">
          <input
            id="project-root"
            name="projectRoot"
            type="text"
            value={root}
            required
            autoComplete="off"
            spellCheck={false}
            aria-describedby={rootTooLong ? "project-root-help project-root-error" : "project-root-help"}
            aria-invalid={rootTooLong || undefined}
            onChange={(event) => setRoot(event.target.value)}
          />
          <button type="submit" disabled={busy || rootTooLong || !root.trim()}>
            {state.pending === "add" ? "Adding…" : "Add root"}
          </button>
        </div>
        {rootTooLong && (
          <p id="project-root-error" className="error-message" role="alert">
            {PROJECT_ROOT_LENGTH_ERROR}
          </p>
        )}
      </form>

      {state.error && (
        <div className="project-artifact-error" role="alert">
          <p>{state.error}</p>
          {state.loadingRoots && (
            <button type="button" className="secondary-button" onClick={state.reload}>
              Reload saved roots
            </button>
          )}
        </div>
      )}

      <div className="project-root-manager" aria-labelledby="saved-project-roots-heading">
        <div className="project-root-manager-heading">
          <h3 id="saved-project-roots-heading">Saved roots</h3>
          <button
            type="button"
            onClick={() => void state.scan()}
            disabled={busy || activeRoots.length === 0}
          >
            {state.scanning ? "Scanning…" : "Scan active roots"}
          </button>
        </div>
        {state.loadingRoots && <p role="status">Loading saved roots.</p>}
        {!state.loadingRoots && state.roots.length === 0 && (
          <p>No roots saved. Add one absolute path to begin.</p>
        )}
        {state.roots.length > 0 && (
          <ul className="project-root-list">
            {state.roots.map((saved) => (
              <li key={saved.id}>
                <div className="project-root-details">
                  <strong>{saved.paused ? "Paused" : "Active"}</strong>
                  <span className="project-artifact-path">{saved.displayPath}</span>
                  <small>
                    Last scan: {saved.lastScannedAtUnixSeconds == null
                      ? "Never"
                      : formatModified(saved.lastScannedAtUnixSeconds)}
                  </small>
                </div>
                <div className="project-root-actions">
                  <button
                    type="button"
                    onClick={() => void state.scan(saved.id)}
                    disabled={busy || saved.paused}
                    aria-label={`Scan ${saved.displayPath}`}
                  >
                    Scan
                  </button>
                  <button
                    type="button"
                    className="secondary-button"
                    onClick={() => void state.setPaused(saved.id, !saved.paused)}
                    disabled={busy}
                    aria-label={`${saved.paused ? "Resume" : "Pause"} ${saved.displayPath}`}
                  >
                    {saved.paused ? "Resume" : "Pause"}
                  </button>
                  <button
                    type="button"
                    className="secondary-button"
                    onClick={() => void state.remove(saved.id)}
                    disabled={busy}
                    aria-label={`Remove ${saved.displayPath}`}
                  >
                    Remove
                  </button>
                </div>
              </li>
            ))}
          </ul>
        )}
        <p className="project-root-note">Removing a root only forgets it. Project files stay unchanged.</p>
      </div>

      <div className="project-artifact-status" role="status" aria-live="polite">
        {!state.attempted && !state.scanning && <p>Choose Scan when you want to inspect saved roots.</p>}
        {state.scanning && <p>Scanning project roots without changing files.</p>}
        {state.result && state.result.records.length === 0 && (
          <p>No marker-backed project artifacts were found.</p>
        )}
        {state.result && state.result.records.length > 0 && (
          <p>
            {state.result.records.length} rebuildable{" "}
            {state.result.records.length === 1 ? "artifact" : "artifacts"} found. All are not
            selected.
          </p>
        )}
      </div>

      {state.result && state.result.diagnostics.length > 0 && (
        <p className="project-artifact-diagnostics">
          {state.result.diagnostics.length}{" "}
          {state.result.diagnostics.length === 1 ? "location was" : "locations were"} skipped or
          suppressed to avoid overlap.
        </p>
      )}

      {groups.length > 0 && (
        <ul className="project-artifact-groups" aria-label="Discovered project artifacts">
          {groups.map((group) => (
            <li key={group.key}>
              <header>
                <div>
                  <h3>{group.projectName}</h3>
                  <p className="project-artifact-path">{group.projectPath}</p>
                </div>
                <strong>{ECOSYSTEM_LABELS[group.ecosystem]}</strong>
              </header>
              <ul className="project-artifact-records">
                {group.records.map((record) => (
                  <li key={record.id}>
                    <h4>{ARTIFACT_LABELS[record.artifact.artifactType]}</h4>
                    <p className="project-artifact-path">{record.displayPath}</p>
                    <dl>
                      <div><dt>Size</dt><dd>{formatBytes(record.bytes)}</dd></div>
                      <div><dt>Age</dt><dd>{formatAge(record.ageSeconds)}</dd></div>
                      <div><dt>Modified</dt><dd>{record.modifiedUnixSeconds == null ? "Unavailable" : formatModified(record.modifiedUnixSeconds)}</dd></div>
                      <div><dt>Activity</dt><dd>{record.activity === "inUse" ? "In use" : "Idle"}</dd></div>
                      <div><dt>Confidence</dt><dd>{record.artifact.confidence === "high" ? "High" : "Medium"}</dd></div>
                      <div><dt>Risk</dt><dd>{record.risk === "safe" ? "Safe" : record.risk === "highImpact" ? "High impact" : "Recoverable"}</dd></div>
                      <div><dt>Recoverability</dt><dd>Rebuildable</dd></div>
                      <div><dt>Rebuild consequence</dt><dd>{CONSEQUENCE_LABELS[record.artifact.rebuildConsequence]}</dd></div>
                      <div><dt>Selection</dt><dd>Not selected</dd></div>
                    </dl>
                  </li>
                ))}
              </ul>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
