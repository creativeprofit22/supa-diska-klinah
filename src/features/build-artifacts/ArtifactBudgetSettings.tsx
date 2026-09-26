import { useEffect, useState } from "react";
import type { ArtifactBudgetPolicy, BudgetLimits, ProjectBudgetOverride } from "./api";
import { useBuildArtifacts } from "./useBuildArtifacts";
import { useProjectRoots } from "./useProjectRoots";
import { useStrings } from "../../shared/i18n/I18nProvider";
import { buildArtifactsStrings } from "./strings";

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
  const strings = useStrings(buildArtifactsStrings);
  const t = strings.settings;
  const state = useBuildArtifacts();
  const { roots, status: rootStatus, error: rootError, reload: reloadRoots } = useProjectRoots();
  const [draft, setDraft] = useState<ArtifactBudgetPolicy | null>(null);

  useEffect(() => {
    if (state.policy) setDraft(state.policy);
  }, [state.policy]);


  if (!draft) {
    return <div className="settings-panel artifact-budget-settings"><h2>{strings.heading}</h2><p role="status">{t.loading}</p></div>;
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
      <h2>{strings.heading}</h2>
      <p>{t.intro}</p>
      <form onSubmit={(event) => { event.preventDefault(); if (!invalidExplicitOverride) void state.savePolicy(draft); }}>
        <label className="toggle-row">
          <span><strong>{t.enforce}</strong><small>{t.enforceHelp}</small></span>
          <input type="checkbox" checked={draft.enabled} disabled={Boolean(state.pending)} onChange={(event) => setDraft({ ...draft, enabled: event.target.checked })} />
        </label>
        <div className="settings-number-grid">
          <label>
            {t.globalSize}
            <input type="number" min="0.01" step="0.01" inputMode="decimal" value={display(draft.globalLimits.maximumAllocatedBytes, GIB)} onChange={(event) => setDraft({ ...draft, globalLimits: { ...draft.globalLimits, maximumAllocatedBytes: optionalNumber(event.target.value, GIB) } })} />
            <small>{t.globalSizeHelp}</small>
          </label>
          <label>
            {t.globalAge}
            <input type="number" min="1" step="1" inputMode="numeric" value={display(draft.globalLimits.maximumAgeSeconds, DAY)} onChange={(event) => setDraft({ ...draft, globalLimits: { ...draft.globalLimits, maximumAgeSeconds: optionalNumber(event.target.value, DAY) } })} />
            <small>{t.globalAgeHelp}</small>
          </label>
          <label>
            {t.interval}
            <input required type="number" min="1" max="168" step="1" inputMode="numeric" value={draft.scheduledAnalysisIntervalSeconds / HOUR} onChange={(event) => setDraft({ ...draft, scheduledAnalysisIntervalSeconds: Number(event.target.value) * HOUR })} />
          </label>
          <label>
            {t.grace}
            <input required type="number" min="1" max="720" step="1" inputMode="numeric" value={draft.staleChangeGraceSeconds / HOUR} onChange={(event) => setDraft({ ...draft, staleChangeGraceSeconds: Number(event.target.value) * HOUR })} />
          </label>
          <label>
            {t.recovery}
            <input required type="number" min="1" max="30" step="1" inputMode="numeric" value={draft.quarantineGraceSeconds / DAY} onChange={(event) => setDraft({ ...draft, quarantineGraceSeconds: Number(event.target.value) * DAY })} />
          </label>
        </div>

        {rootStatus === "loading" && <p role="status">{strings.loadingRoots}</p>}
        {rootError && <>
          <p className="build-artifact-error" role="alert">{rootError}</p>
          <button type="button" className="secondary-button" onClick={() => void reloadRoots()}>{strings.retryRoots}</button>
        </>}
        <fieldset className="project-budget-overrides" disabled={rootStatus !== "ready"}>
          <legend>{t.perProject}</legend>
          {rootStatus === "ready" && roots.length === 0 && <p>{t.noRoots}</p>}
          {roots.map((root) => {
            const saved = draft.projectOverrides.find((entry) => entry.rootId === root.id)?.policy ?? { mode: "inherit" as const };
            const invalid = saved.mode === "explicit" && !hasEnabledLimit(saved.limits);
            const errorId = `artifact-budget-${root.id}-error`;
            return <div className="project-budget-row" key={root.id}>
              <label>
                <span className="project-path-label">{root.displayPath}</span>
                {t.behavior}
                <select value={saved.mode} onChange={(event) => {
                  const mode = event.target.value;
                  if (mode === "explicit") updateOverride(root.id, { mode, limits: draft.globalLimits });
                  else updateOverride(root.id, { mode: mode as "inherit" | "disabled" });
                }}>
                  <option value="inherit">{t.modeInherit}</option>
                  <option value="disabled">{t.modeDisabled}</option>
                  <option value="explicit">{t.modeExplicit}</option>
                </select>
              </label>
              {saved.mode === "explicit" && <div className="project-budget-limits">
                <label>
                  {t.maximumSize}
                  <input type="number" min="0.01" step="0.01" inputMode="decimal" value={display(saved.limits.maximumAllocatedBytes, GIB)} aria-invalid={invalid || undefined} aria-describedby={invalid ? errorId : undefined} onChange={(event) => updateOverride(root.id, { mode: "explicit", limits: limitsFrom(event.target.value, display(saved.limits.maximumAgeSeconds, DAY)) })} />
                </label>
                <label>
                  {t.maximumAge}
                  <input type="number" min="1" step="1" inputMode="numeric" value={display(saved.limits.maximumAgeSeconds, DAY)} aria-invalid={invalid || undefined} aria-describedby={invalid ? errorId : undefined} onChange={(event) => updateOverride(root.id, { mode: "explicit", limits: limitsFrom(display(saved.limits.maximumAllocatedBytes, GIB), event.target.value) })} />
                </label>
                {invalid && <p id={errorId} className="error-message" role="alert">{t.emptyExplicit}</p>}
              </div>}
            </div>;
          })}
        </fieldset>
        {state.error && <p className="build-artifact-error" role="alert">{state.error}</p>}
        <p className="settings-note">{t.recoveryNote}</p>
        <div className="button-row"><button type="submit" disabled={Boolean(state.pending) || invalidExplicitOverride}>{state.pending === "policy" ? t.saving : t.save}</button></div>
      </form>
    </div>
  );
}
