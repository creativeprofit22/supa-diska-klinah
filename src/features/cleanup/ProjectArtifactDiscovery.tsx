import { useState, type FormEvent } from "react";
import { formatBytes, formatModified } from "./format";
import { useProjectArtifactDiscovery } from "./model/useProjectArtifactDiscovery";

const MAX_PROJECT_ROOT_BYTES = 4_096;
const PROJECT_ROOT_LENGTH_ERROR = "Project root must be 4,096 UTF-8 bytes or fewer.";

export function ProjectArtifactDiscovery() {
  const [root, setRoot] = useState("");
  const state = useProjectArtifactDiscovery();
  const rootTooLong = new TextEncoder().encode(root).length > MAX_PROJECT_ROOT_BYTES;

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!state.loading && !rootTooLong && root.trim()) void state.scan(root);
  }

  return (
    <section className="project-artifacts" aria-labelledby="project-artifacts-heading">
      <div className="project-artifacts-header">
        <div>
          <p className="kicker">Developer storage</p>
          <h2 id="project-artifacts-heading">Coding project artifacts</h2>
        </div>
        <p id="project-root-help">
          Inspect marker-backed, rebuildable dependencies. This scan never selects or removes them.
        </p>
      </div>

      <form className="project-artifact-form" onSubmit={submit}>
        <label htmlFor="project-root">Project root</label>
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
          <button type="submit" disabled={state.loading || rootTooLong}>
            {state.loading ? "Scanning…" : "Scan project root"}
          </button>
        </div>
        {rootTooLong && (
          <p id="project-root-error" className="error-message" role="alert">
            {PROJECT_ROOT_LENGTH_ERROR}
          </p>
        )}
      </form>

      <div className="project-artifact-status" role="status" aria-live="polite">
        {!state.attempted && <p>Paste one explicit Windows project path to inspect it.</p>}
        {state.loading && <p>Scanning the project root without changing files.</p>}
        {state.error && (
          <div>
            <p>Project artifacts could not be scanned. Check the root and try again.</p>
            <button type="button" onClick={state.retry}>Try project scan again</button>
          </div>
        )}
        {state.result && state.result.records.length === 0 && (
          <p>No marker-backed Node.js dependency folders were found.</p>
        )}
        {state.result && state.result.records.length > 0 && (
          <p>
            {state.result.records.length} rebuildable {state.result.records.length === 1 ? "artifact" : "artifacts"} found.
          </p>
        )}
      </div>

      {state.result && state.result.diagnostics.length > 0 && (
        <p className="project-artifact-diagnostics">
          {state.result.diagnostics.length} {state.result.diagnostics.length === 1 ? "location was" : "locations were"} skipped.
        </p>
      )}

      {state.result && state.result.records.length > 0 && (
        <ul className="project-artifact-records" aria-label="Discovered project artifacts">
          {state.result.records.map((record) => (
            <li key={record.id}>
              <h3>{record.projectName}</h3>
              <p className="project-artifact-path">{record.projectPath}</p>
              <dl>
                <div><dt>Ecosystem</dt><dd>Node.js</dd></div>
                <div><dt>Artifact</dt><dd>Installed dependencies</dd></div>
                <div><dt>Size</dt><dd>{formatBytes(record.bytes)}</dd></div>
                <div><dt>Artifact modified</dt><dd>{record.modifiedUnixSeconds == null ? "Unavailable" : formatModified(record.modifiedUnixSeconds)}</dd></div>
                <div><dt>Risk</dt><dd>Recoverable</dd></div>
                <div><dt>Recoverability</dt><dd>Rebuildable</dd></div>
                <div><dt>Rebuild consequence</dt><dd>Network download required</dd></div>
              </dl>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
