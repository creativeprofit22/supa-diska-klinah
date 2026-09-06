import { useEffect, useState } from "react";
import type { ArtifactBudgetPolicy, BudgetLimits, ProjectBudgetOverride } from "./api";
import { useBuildArtifacts } from "./useBuildArtifacts";
import { useProjectRoots } from "./useProjectRoots";

const GIB = 1024 ** 3;
const DAY = 86_400;
const HOUR = 3_600;

function optionalNumber(value: string, multiplier: number): number | null {
  if (!value) return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed > 0 ? Math.round(parsed * multiplier) : null;
}

function display(value: number | null | undefined, divisor: number): string {
  return value == null ? "" : String(value / divisor);
}

function limitsFrom(size: string, age: string): BudgetLimits {
  return {
    maximumAllocatedBytes: optionalNumber(size, GIB),
    maximumAgeSeconds: optionalNumber(age, DAY),
  };
}

function hasEnabledLimit(limits: BudgetLimits): boolean {
  return (limits.maximumAllocatedBytes ?? 0) > 0 || (limits.maximumAgeSeconds ?? 0) > 0;
}

export function ArtifactBudgetSettings() {
  const state = useBuildArtifacts();
  const { roots, status: rootStatus, error: rootError, reload: reloadRoots } = useProjectRoots();
  const [draft, setDraft] = useState<ArtifactBudgetPolicy | null>(null);

  useEffect(() => {
    if (state.policy) setDraft(state.policy);
  }, [state.policy]);


  if (!draft) {
    return <div className="settings-panel artifact-budget-settings"><h2>Build artifact budgets</h2><p role="status">Loading artifact settings…</p></div>;
  }

  const updateOverride = (rootId: string, policy: ProjectBudgetOverride) => {
    setDraft({
      ...draft,
      projectOverrides: [
        ...draft.projectOverrides.filter((entry) => entry.rootId !== rootId),
        { rootId, policy },
      ],
    });
  };
  const invalidExplicitOverride = draft.projectOverrides.some(
    (entry) => entry.policy.mode === "explicit" && !hasEnabledLimit(entry.policy.limits),
  );

  return (
    <div className="settings-panel artifact-budget-settings" aria-busy={Boolean(state.pending)}>
      <h2>Build artifact budgets</h2>
      <p>Disabled by default. Analysis never treats watcher silence as a successful build.</p>
      <form onSubmit={(event) => { event.preventDefault(); if (!invalidExplicitOverride) void state.savePolicy(draft); }}>
        <label className="toggle-row">
          <span><strong>Enforce artifact budgets</strong><small>When off, scheduled analysis and artifact quarantine stop.</small></span>
          <input type="checkbox" checked={draft.enabled} disabled={Boolean(state.pending)} onChange={(event) => setDraft({ ...draft, enabled: event.target.checked })} />
        </label>
        <div className="settings-number-grid">
          <label>
            Global maximum size (GiB)
            <input type="number" min="0.01" step="0.01" inputMode="decimal" value={display(draft.globalLimits.maximumAllocatedBytes, GIB)} onChange={(event) => setDraft({ ...draft, globalLimits: { ...draft.globalLimits, maximumAllocatedBytes: optionalNumber(event.target.value, GIB) } })} />
            <small>Leave empty for no global size limit.</small>
          </label>
          <label>
            Global maximum age (days)
            <input type="number" min="1" step="1" inputMode="numeric" value={display(draft.globalLimits.maximumAgeSeconds, DAY)} onChange={(event) => setDraft({ ...draft, globalLimits: { ...draft.globalLimits, maximumAgeSeconds: optionalNumber(event.target.value, DAY) } })} />
            <small>Leave empty for no global age limit.</small>
          </label>
          <label>
            Scheduled analysis interval (hours)
            <input required type="number" min="1" max="168" step="1" inputMode="numeric" value={draft.scheduledAnalysisIntervalSeconds / HOUR} onChange={(event) => setDraft({ ...draft, scheduledAnalysisIntervalSeconds: Number(event.target.value) * HOUR })} />
          </label>
          <label>
            External-change grace (hours)
            <input required type="number" min="1" max="720" step="1" inputMode="numeric" value={draft.staleChangeGraceSeconds / HOUR} onChange={(event) => setDraft({ ...draft, staleChangeGraceSeconds: Number(event.target.value) * HOUR })} />
          </label>
          <label>
            Quarantine recovery (days)
            <input required type="number" min="1" max="30" step="1" inputMode="numeric" value={draft.quarantineGraceSeconds / DAY} onChange={(event) => setDraft({ ...draft, quarantineGraceSeconds: Number(event.target.value) * DAY })} />
          </label>
        </div>

        {rootStatus === "loading" && <p role="status">Loading project roots…</p>}
        {rootError && <>
          <p className="build-artifact-error" role="alert">{rootError}</p>
          <button type="button" className="secondary-button" onClick={() => void reloadRoots()}>Retry loading project roots</button>
        </>}
        <fieldset className="project-budget-overrides" disabled={rootStatus !== "ready"}>
          <legend>Per-project behavior</legend>
          {rootStatus === "ready" && roots.length === 0 && <p>Add a project root on Cleanup to set a project override.</p>}
          {roots.map((root) => {
            const saved = draft.projectOverrides.find((entry) => entry.rootId === root.id)?.policy ?? { mode: "inherit" as const };
            const invalid = saved.mode === "explicit" && !hasEnabledLimit(saved.limits);
            const errorId = `artifact-budget-${root.id}-error`;
            return <div className="project-budget-row" key={root.id}>
              <label>
                <span className="project-path-label">{root.displayPath}</span>
                Budget behavior
                <select value={saved.mode} onChange={(event) => {
                  const mode = event.target.value;
                  if (mode === "explicit") updateOverride(root.id, { mode, limits: draft.globalLimits });
                  else updateOverride(root.id, { mode: mode as "inherit" | "disabled" });
                }}>
                  <option value="inherit">Use global limits</option>
                  <option value="disabled">Disable for this project</option>
                  <option value="explicit">Use project limits</option>
                </select>
              </label>
              {saved.mode === "explicit" && <div className="project-budget-limits">
                <label>
                  Maximum size (GiB)
                  <input type="number" min="0.01" step="0.01" inputMode="decimal" value={display(saved.limits.maximumAllocatedBytes, GIB)} aria-invalid={invalid || undefined} aria-describedby={invalid ? errorId : undefined} onChange={(event) => updateOverride(root.id, { mode: "explicit", limits: limitsFrom(event.target.value, display(saved.limits.maximumAgeSeconds, DAY)) })} />
                </label>
                <label>
                  Maximum age (days)
                  <input type="number" min="1" step="1" inputMode="numeric" value={display(saved.limits.maximumAgeSeconds, DAY)} aria-invalid={invalid || undefined} aria-describedby={invalid ? errorId : undefined} onChange={(event) => updateOverride(root.id, { mode: "explicit", limits: limitsFrom(display(saved.limits.maximumAllocatedBytes, GIB), event.target.value) })} />
                </label>
                {invalid && <p id={errorId} className="error-message" role="alert">Enter a maximum size or maximum age for project limits.</p>}
              </div>}
            </div>;
          })}
        </fieldset>
        {state.error && <p className="build-artifact-error" role="alert">{state.error}</p>}
        <p className="settings-note">Quarantined generations remain undoable until the recovery period ends.</p>
        <div className="button-row"><button type="submit" disabled={Boolean(state.pending) || invalidExplicitOverride}>{state.pending === "policy" ? "Saving…" : "Save artifact budgets"}</button></div>
      </form>
    </div>
  );
}
