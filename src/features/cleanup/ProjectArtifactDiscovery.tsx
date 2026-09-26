import { useMemo, useState, type FormEvent } from "react";
import { useFormat, useStrings } from "../../shared/i18n/I18nProvider";
import {
  type ArtifactEcosystem,
  type ProjectArtifactRecord,
} from "./api/previewCleanup";
import { formatModified } from "./format";
import { useProjectArtifactDiscovery } from "./model/useProjectArtifactDiscovery";
import { cleanupStrings, type CleanupStrings } from "./strings";

const MAX_PROJECT_ROOT_BYTES = 4_096;
// Ecosystem and product names stay untranslated.
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
function formatAge(t: CleanupStrings["projects"], seconds?: number | null): string {
  if (seconds == null) return t.unavailable;
  if (seconds < 60) return t.ageSeconds(seconds);
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return t.ageMinutes(minutes);
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return t.ageHours(hours);
  const days = Math.floor(hours / 24);
  return t.ageDays(days);
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
  const t = useStrings(cleanupStrings).projects;
  const fmt = useFormat();
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
          <p className="kicker">{t.kicker}</p>
          <h2 id="project-artifacts-heading">{t.heading}</h2>
        </div>
        <p id="project-root-help">{t.help}</p>
      </div>

      <form className="project-artifact-form" onSubmit={submit}>
        <label htmlFor="project-root">{t.addLabel}</label>
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
            {state.pending === "add" ? t.adding : t.addRoot}
          </button>
        </div>
        {rootTooLong && (
          <p id="project-root-error" className="error-message" role="alert">
            {t.rootTooLong(fmt.number(MAX_PROJECT_ROOT_BYTES))}
          </p>
        )}
      </form>

      {state.error && (
        <div className="project-artifact-error" role="alert">
          <p>{state.error}</p>
          {state.loadingRoots && (
            <button type="button" className="secondary-button" onClick={state.reload}>
              {t.reloadRoots}
            </button>
          )}
        </div>
      )}

      <section className="project-root-manager" aria-labelledby="saved-project-roots-heading">
        <div className="project-root-manager-heading">
          <h3 id="saved-project-roots-heading">{t.savedRoots}</h3>
          <button
            type="button"
            onClick={() => void state.scan()}
            disabled={busy || activeRoots.length === 0}
          >
            {state.scanning ? t.scanning : t.scanActive}
          </button>
        </div>
        {state.loadingRoots && <p role="status">{t.loadingRoots}</p>}
        {!state.loadingRoots && state.roots.length === 0 && (
          <p>{t.noRoots}</p>
        )}
        {state.roots.length > 0 && (
          <ul className="project-root-list">
            {state.roots.map((saved) => (
              <li key={saved.id}>
                <div className="project-root-details">
                  <strong>{saved.paused ? t.paused : t.active}</strong>
                  <span className="project-artifact-path">{saved.displayPath}</span>
                  <small>
                    {t.lastScan}{saved.lastScannedAtUnixSeconds == null
                      ? t.never
                      : formatModified(saved.lastScannedAtUnixSeconds, fmt.locale)}
                  </small>
                </div>
                <div className="project-root-actions">
                  <button
                    type="button"
                    onClick={() => void state.scan(saved.id)}
                    disabled={busy || saved.paused}
                    aria-label={t.scanRoot(saved.displayPath)}
                  >
                    {t.scan}
                  </button>
                  <button
                    type="button"
                    className="secondary-button"
                    onClick={() => void state.setPaused(saved.id, !saved.paused)}
                    disabled={busy}
                    aria-label={saved.paused ? t.resumeRoot(saved.displayPath) : t.pauseRoot(saved.displayPath)}
                  >
                    {saved.paused ? t.resume : t.pause}
                  </button>
                  <button
                    type="button"
                    className="secondary-button"
                    onClick={() => void state.remove(saved.id)}
                    disabled={busy}
                    aria-label={t.removeRoot(saved.displayPath)}
                  >
                    {t.remove}
                  </button>
                </div>
              </li>
            ))}
          </ul>
        )}
        <p className="project-root-note">{t.removeNote}</p>
      </section>

      <div className="project-artifact-status" role="status" aria-live="polite">
        {!state.attempted && !state.scanning && <p>{t.idle}</p>}
        {state.scanning && <p>{t.scanningRoots}</p>}
        {state.result && state.result.records.length === 0 && (
          <p>{t.noArtifacts}</p>
        )}
        {state.result && state.result.records.length > 0 && (
          <p>{t.found(state.result.records.length)}</p>
        )}
      </div>

      {state.result && state.result.diagnostics.length > 0 && (
        <p className="project-artifact-diagnostics">
          {t.diagnostics(state.result.diagnostics.length)}
        </p>
      )}

      {groups.length > 0 && (
        <ul className="project-artifact-groups" aria-label={t.listLabel}>
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
                    <h4>{t.artifacts[record.artifact.artifactType]}</h4>
                    <p className="project-artifact-path">{record.displayPath}</p>
                    <dl>
                      <div><dt>{t.size}</dt><dd>{fmt.bytes(record.bytes)}</dd></div>
                      <div><dt>{t.age}</dt><dd>{formatAge(t, record.ageSeconds)}</dd></div>
                      <div><dt>{t.modified}</dt><dd>{record.modifiedUnixSeconds == null ? t.unavailable : formatModified(record.modifiedUnixSeconds, fmt.locale)}</dd></div>
                      <div><dt>{t.activity}</dt><dd>{record.activity === "inUse" ? t.inUse : t.idleActivity}</dd></div>
                      <div><dt>{t.confidence}</dt><dd>{record.artifact.confidence === "high" ? t.high : t.medium}</dd></div>
                      <div><dt>{t.risk}</dt><dd>{record.risk === "safe" ? t.safe : record.risk === "highImpact" ? t.highImpact : t.recoverable}</dd></div>
                      <div><dt>{t.recoverability}</dt><dd>{t.rebuildable}</dd></div>
                      <div><dt>{t.rebuildConsequence}</dt><dd>{t.consequences[record.artifact.rebuildConsequence]}</dd></div>
                      <div><dt>{t.selection}</dt><dd>{t.notSelected}</dd></div>
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
