import { useEffect, useMemo, useState } from "react";
import { useFormat, useStrings } from "../../shared/i18n/I18nProvider";
import { buildArtifactsStrings } from "./strings";
import type {
  ArtifactRole,
  BuildEcosystem,
  RebuildCost,
  RegisterBuildProfileInput,
} from "./api";
import { useBuildArtifacts } from "./useBuildArtifacts";
import { useProjectRoots } from "./useProjectRoots";
import { artifactPathErrors, textError, MAX_PROFILE_ARGUMENTS, MAX_PROFILE_ARTIFACTS, MAX_ARGUMENT_BYTES, MAX_STRING_BYTES } from "./profileValidation";

function FieldError({ id, error }: { id: string; error: string | undefined }) {
  return error ? <small id={id} className="build-artifact-error" role="alert">{error}</small> : null;
}

export function BuildArtifactCoordinator() {
  const strings = useStrings(buildArtifactsStrings);
  const t = strings.coordinator;
  const fmt = useFormat();
  const state = useBuildArtifacts();
  const { roots, status: rootStatus, error: rootError, reload: reloadRoots } = useProjectRoots();
  const [displayName, setDisplayName] = useState("");
  const [rootId, setRootId] = useState("");
  const [ecosystem, setEcosystem] = useState<BuildEcosystem>("rust");
  const [executable, setExecutable] = useState("");
  const [workingDirectory, setWorkingDirectory] = useState("");
  const [profileLabel, setProfileLabel] = useState("debug");
  const [toolchainLabel, setToolchainLabel] = useState("stable");
  const [targetLabel, setTargetLabel] = useState("default");
  const [rebuildCost, setRebuildCost] = useState<RebuildCost>("medium");
  const [argv, setArgv] = useState(["build"]);
  const [artifacts, setArtifacts] = useState([
    { relativePath: "", role: "generation" as ArtifactRole },
  ]);

  useEffect(() => {
    setRootId((current) => current || roots[0]?.id || "");
  }, [roots]);

  const generations = useMemo(
    () => new Map(state.preview?.generations.map((item) => [item.generationId, item]) ?? []),
    [state.preview],
  );

  const labels = [displayName, profileLabel, toolchainLabel, targetLabel];
  // Leave untouched empty fields to native required validation, without announcing errors on load.
  const labelErrors = labels.map((value) => value ? textError(value, MAX_STRING_BYTES, true, strings.validation) : undefined);
  const argumentErrors = argv.map((value) => textError(value, MAX_ARGUMENT_BYTES, false, strings.validation));
  const pathErrors = artifactPathErrors(artifacts.map((artifact) => artifact.relativePath), strings.validation)
    .map((error, index) => artifacts[index].relativePath ? error : undefined);
  const invalid = [...labelErrors, ...argumentErrors, ...pathErrors].some(Boolean)
    || labels.some((value) => !value) || artifacts.some((artifact) => !artifact.relativePath)
    || argv.length > MAX_PROFILE_ARGUMENTS || artifacts.length === 0 || artifacts.length > MAX_PROFILE_ARTIFACTS;

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    if (invalid) return;
    const input: RegisterBuildProfileInput = {
      rootId,
      displayName,
      ecosystem,
      executable,
      argv,
      workingDirectory,
      profileLabel,
      toolchainLabel,
      targetLabel,
      rebuildCost,
      artifactPaths: artifacts,
    };
    const saved = await state.register(input);
    if (saved) setDisplayName("");
  }

  return (
    <section className="build-artifacts" aria-labelledby="build-artifacts-heading">
      <header className="build-artifacts-header">
        <div>
          <h2 id="build-artifacts-heading">{strings.heading}</h2>
          <p>{t.intro}</p>
        </div>
        <p>{state.policy?.enabled ? t.budgetsEnabled : t.budgetsDisabled}</p>
      </header>

      <form className="build-profile-form" onSubmit={(event) => void submit(event)}>
        {rootStatus === "loading" && <p role="status">{strings.loadingRoots}</p>}
        {rootError && <>
          <p className="build-artifact-error" role="alert">{rootError}</p>
          <button type="button" className="secondary-button" onClick={() => void reloadRoots()}>{strings.retryRoots}</button>
        </>}
        {rootStatus === "ready" && roots.length === 0 && <p>{t.noRoots}</p>}
        <fieldset disabled={Boolean(state.pending) || rootStatus !== "ready"}>
          <legend>{t.registerLegend}</legend>
          <p className="form-help">
            {t.confirmHelp} {t.labelBytesHelp(MAX_STRING_BYTES)}
          </p>
          <div className="build-form-grid">
            <div>
              <label>
                {t.profileName}
                <input required aria-invalid={Boolean(labelErrors[0])} aria-describedby={labelErrors[0] ? "name-error" : undefined} value={displayName} onChange={(event) => setDisplayName(event.target.value)} />
              </label>
              <FieldError id="name-error" error={labelErrors[0]} />
            </div>
            <label>
              {t.projectRoot}
              <select required value={rootId} onChange={(event) => setRootId(event.target.value)}>
                <option value="" disabled>{t.selectRoot}</option>
                {roots.map((root) => <option key={root.id} value={root.id}>{root.displayPath}</option>)}
              </select>
            </label>
            <label>
              {t.ecosystem}
              <select value={ecosystem} onChange={(event) => setEcosystem(event.target.value as BuildEcosystem)}>
                <option value="rust">{t.ecosystemOptions.rust}</option>
                <option value="node">{t.ecosystemOptions.node}</option>
                <option value="generic">{t.ecosystemOptions.generic}</option>
              </select>
            </label>
            <label className="wide-field">
              {t.executable}
              <input required value={executable} onChange={(event) => setExecutable(event.target.value)} spellCheck={false} />
            </label>
            <label>
              {t.workingDirectory}
              <input value={workingDirectory} onChange={(event) => setWorkingDirectory(event.target.value)} placeholder={t.workingDirectoryPlaceholder} spellCheck={false} />
            </label>
            <div>
              <label>
                {t.profileLabel}
                <input required aria-invalid={Boolean(labelErrors[1])} aria-describedby={labelErrors[1] ? "profile-error" : undefined} value={profileLabel} onChange={(event) => setProfileLabel(event.target.value)} />
              </label>
              <FieldError id="profile-error" error={labelErrors[1]} />
            </div>
            <div>
              <label>
                {t.toolchainLabel}
                <input required aria-invalid={Boolean(labelErrors[2])} aria-describedby={labelErrors[2] ? "toolchain-error" : undefined} value={toolchainLabel} onChange={(event) => setToolchainLabel(event.target.value)} />
              </label>
              <FieldError id="toolchain-error" error={labelErrors[2]} />
            </div>
            <div>
              <label>
                {t.targetLabel}
                <input required aria-invalid={Boolean(labelErrors[3])} aria-describedby={labelErrors[3] ? "target-error" : undefined} value={targetLabel} onChange={(event) => setTargetLabel(event.target.value)} />
              </label>
              <FieldError id="target-error" error={labelErrors[3]} />
            </div>
            <label>
              {t.rebuildCost}
              <select value={rebuildCost} onChange={(event) => setRebuildCost(event.target.value as RebuildCost)}>
                <option value="low">{t.rebuildCostOptions.low}</option><option value="medium">{t.rebuildCostOptions.medium}</option><option value="high">{t.rebuildCostOptions.high}</option>
              </select>
            </label>
          </div>

          <fieldset className="repeatable-fields">
            <legend>{t.argumentsLegend}</legend>
            <p className="form-help">{t.argumentsHelp(MAX_PROFILE_ARGUMENTS, MAX_ARGUMENT_BYTES)}</p>
            {argv.map((argument, index) => (
              <div className="repeatable-row" key={`argument-${index}`}>
                <div>
                  <label>
                    {t.argument(index + 1)}
                    <input aria-invalid={Boolean(argumentErrors[index])} aria-describedby={argumentErrors[index] ? `argument-error-${index}` : undefined} value={argument} onChange={(event) => setArgv(argv.map((item, itemIndex) => itemIndex === index ? event.target.value : item))} />
                  </label>
                  <FieldError id={`argument-error-${index}`} error={argumentErrors[index]} />
                </div>
                <button type="button" className="secondary-button" disabled={argv.length === 1} onClick={() => setArgv(argv.filter((_, itemIndex) => itemIndex !== index))}>{t.remove}</button>
              </div>
            ))}
            <button type="button" className="secondary-button" disabled={argv.length >= MAX_PROFILE_ARGUMENTS} onClick={() => setArgv((current) => current.length < MAX_PROFILE_ARGUMENTS ? [...current, ""] : current)}>{t.addArgument}</button>
          </fieldset>

          <fieldset className="repeatable-fields">
            <legend>{t.artifactsLegend}</legend>
            <p className="form-help">{t.artifactsHelp(MAX_PROFILE_ARTIFACTS)}</p>
            {artifacts.map((artifact, index) => (
              <div className="repeatable-row artifact-row" key={`artifact-${index}`}>
                <div>
                  <label>
                    {t.relativePath(index + 1)}
                    <input required aria-invalid={Boolean(pathErrors[index])} aria-describedby={pathErrors[index] ? `path-error-${index}` : undefined} value={artifact.relativePath} onChange={(event) => setArtifacts(artifacts.map((item, itemIndex) => itemIndex === index ? { ...item, relativePath: event.target.value } : item))} spellCheck={false} />
                  </label>
                  <FieldError id={`path-error-${index}`} error={pathErrors[index]} />
                </div>
                <label>
                  {t.role}
                  <select value={artifact.role} onChange={(event) => setArtifacts(artifacts.map((item, itemIndex) => itemIndex === index ? { ...item, role: event.target.value as ArtifactRole } : item))}>
                    <option value="generation">{t.roleOptions.generation}</option><option value="dependency">{t.roleOptions.dependency}</option><option value="incremental">{t.roleOptions.incremental}</option>
                  </select>
                </label>
                <button type="button" className="secondary-button" disabled={artifacts.length === 1} onClick={() => setArtifacts(artifacts.filter((_, itemIndex) => itemIndex !== index))}>{t.remove}</button>
              </div>
            ))}
            <button type="button" className="secondary-button" disabled={artifacts.length >= MAX_PROFILE_ARTIFACTS} onClick={() => setArtifacts((current) => current.length < MAX_PROFILE_ARTIFACTS ? [...current, { relativePath: "", role: "generation" }] : current)}>{t.addArtifactPath}</button>
          </fieldset>
          <button type="submit" disabled={invalid}>{t.submit}</button>
        </fieldset>
      </form>

      {state.error && <p className="build-artifact-error" role="alert">{state.error}</p>}
      <p className="build-run-status" role="status" aria-live="polite">
        {state.run ? `${t.latestBuild(t.runStates[state.run.state])}${state.run.exitCode != null ? t.exitCode(state.run.exitCode) : ""}` : t.noRun}
      </p>

      <section className="saved-build-profiles" aria-labelledby="saved-build-profiles-heading">
        <h3 id="saved-build-profiles-heading">{t.savedProfiles}</h3>
        {state.loading && <p role="status">{t.loadingProfiles}</p>}
        {!state.loading && state.profiles.length === 0 && <p>{t.noProfiles}</p>}
        <ul>
          {state.profiles.map((profile) => {
            const running = state.run?.profileId === profile.profileId && !["succeeded", "failed", "cancelled", "analysisFailed"].includes(state.run.state);
            return <li key={profile.profileId}>
              <div>
                <strong>{profile.displayName}</strong><small>{t.ecosystemValues[profile.ecosystem]} · {profile.profileLabel} · {profile.targetLabel}</small>
                <details className="build-profile-details">
                  <summary aria-label={t.detailsFor(profile.displayName)}>{t.details}</summary>
                  <dl>
                    <dt>{t.executableTerm}</dt><dd><code>{profile.executable}</code></dd>
                    <dt>{t.argumentsLegend}</dt>
                    <dd>{profile.argv.length === 0 ? t.noArguments : <ol aria-label={t.argumentsLegend}>
                      {profile.argv.map((argument, index) => <li key={index}><code>{argument}</code>{argument === "" && <span>{t.emptyArgument}</span>}</li>)}
                    </ol>}</dd>
                    <dt>{t.workingDirectory}</dt><dd><code>{profile.workingDirectory || "."}</code>{profile.workingDirectory === "" && t.projectRootSuffix}</dd>
                    <dt>{t.rebuildCost}</dt><dd>{t.rebuildCostValues[profile.rebuildCost]}</dd>
                    <dt>{t.artifactPathsTerm}</dt>
                    <dd><ul aria-label={t.artifactPathsList}>
                      {profile.artifactPaths.map((artifact) => <li key={artifact.relativePath}><code>{artifact.relativePath}</code> — <span>{t.roleValues[artifact.role]}</span></li>)}
                    </ul></dd>
                  </dl>
                </details>
              </div>
              <div className="button-row">
                <button type="button" disabled={Boolean(state.pending) || state.buildActionsDisabled} onClick={() => void state.start(profile.profileId)}>{t.run}</button>
                {running && <button type="button" className="secondary-button" disabled={state.pending === "cancel"} onClick={() => void state.cancel()}>{t.cancel}</button>}
                <button type="button" className="danger-button" disabled={Boolean(state.pending) || state.buildActionsDisabled} aria-describedby={`forget-${profile.profileId}`} onClick={() => void state.remove(profile.profileId)}>{t.forget}</button>
              </div>
              <small id={`forget-${profile.profileId}`}>{t.forgetHelp}</small>
            </li>;
          })}
        </ul>
      </section>

      <section className="budget-preview" aria-labelledby="budget-preview-heading">
        <div className="project-root-manager-heading">
          <h3 id="budget-preview-heading">{t.budgetPreview}</h3>
          <button type="button" className="secondary-button" disabled={state.loading} onClick={state.reload}>{t.refreshPreview}</button>
        </div>
        {state.preview && <>
          <dl className="cleanup-accounting">
            <div><dt>{t.current}</dt><dd>{fmt.bytes(state.preview.decision.currentAllocatedBytes)}</dd></div>
            <div><dt>{t.projected}</dt><dd>{fmt.bytes(state.preview.decision.projectedAllocatedBytes)}</dd></div>
            <div><dt>{t.quarantine}</dt><dd>{fmt.bytes(state.preview.decision.quarantineBytes)}</dd></div>
          </dl>
          {state.preview.decision.unsatisfiedProtectedByteFloor != null && <p className="protected-floor" role="status">{t.protectedFloorGlobal(fmt.bytes(state.preview.decision.unsatisfiedProtectedByteFloor))}</p>}
          <h4>{t.perProjectPreview}</h4>
          {Array.from(new Set([
            ...roots.map((root) => root.id),
            ...Object.keys(state.preview.decision.projectCurrentBytes),
            ...Object.keys(state.preview.decision.projectProjectedBytes),
            ...Object.keys(state.preview.decision.projectProtectedByteFloors),
          ])).map((projectId) => {
            const decision = state.preview?.decision;
            if (!decision) return null;
            const path = roots.find((root) => root.id === projectId)?.displayPath ?? projectId;
            const override = state.policy?.projectOverrides.find((entry) => entry.rootId === projectId)?.policy;
            const mode = override?.mode ?? "inherit";
            const limits = override?.mode === "explicit" ? override.limits : mode === "inherit" ? state.policy?.globalLimits : undefined;
            const maximum = limits?.maximumAllocatedBytes;
            const floor = decision.projectProtectedByteFloors[projectId] ?? 0;
            return <section key={projectId} aria-label={path}>
              <h5 className="project-path-label">{path}</h5>
              <p>{mode === "explicit" ? t.modeExplicit : mode === "disabled" ? t.modeDisabled : t.modeInherit}</p>
              {mode !== "disabled" && <p className="form-help">
                {t.sizeLimit} {maximum == null ? t.noLimit : fmt.bytes(maximum)} · {t.ageLimit} {limits?.maximumAgeSeconds == null ? t.noLimit : t.days(limits.maximumAgeSeconds / 86_400)}
              </p>}
              <dl className="cleanup-accounting">
                <div><dt>{t.current}</dt><dd>{fmt.bytes(decision.projectCurrentBytes[projectId] ?? 0)}</dd></div>
                <div><dt>{t.projected}</dt><dd>{fmt.bytes(decision.projectProjectedBytes[projectId] ?? 0)}</dd></div>
                <div><dt>{t.protectedFloor}</dt><dd>{fmt.bytes(floor)}</dd></div>
              </dl>
              {maximum != null && floor > maximum && <p className="protected-floor" role="status">{t.protectedFloorProject(fmt.bytes(maximum))}</p>}
            </section>;
          })}
          <h4>{t.selectedGenerations}</h4>
          {state.preview.decision.selectedGenerationIds.length === 0 ? <p>{t.nothingSelected}</p> : <ul>{state.preview.decision.selectedGenerationIds.map((id) => <li key={id}><strong>{generations.get(id)?.normalizedPath ?? id}</strong><span>{fmt.bytes(generations.get(id)?.allocatedBytes ?? 0)}</span></li>)}</ul>}
          <h4>{t.protectedGenerations}</h4>
          {state.preview.decision.protected.length === 0 ? <p>{t.noProtected}</p> : <ul className="protected-generation-list">{state.preview.decision.protected.map((item) => <li key={item.generationId}><strong>{generations.get(item.generationId)?.normalizedPath ?? item.generationId}</strong><span>{item.reasons.map((reason) => t.reasons[reason]).join(t.reasonSeparator)}</span></li>)}</ul>}
        </>}
      </section>
    </section>
  );
}
