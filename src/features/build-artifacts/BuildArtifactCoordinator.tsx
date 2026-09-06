import { useEffect, useMemo, useState } from "react";
import { formatBytes } from "../cleanup/format";
import type {
  ArtifactRole,
  BuildEcosystem,
  ProtectionReason,
  RebuildCost,
  RegisterBuildProfileInput,
  BuildRunState,
} from "./api";
import { useBuildArtifacts } from "./useBuildArtifacts";
import { useProjectRoots } from "./useProjectRoots";
import { artifactPathErrors, textError, MAX_PROFILE_ARGUMENTS, MAX_PROFILE_ARTIFACTS, MAX_ARGUMENT_BYTES, MAX_STRING_BYTES } from "./profileValidation";

function FieldError({ id, error }: { id: string; error: string | undefined }) {
  return error ? <small id={id} className="build-artifact-error" role="alert">{error}</small> : null;
}

const runStateLabels: Record<BuildRunState, string> = {
  queued: "queued",
  running: "running",
  succeeded: "succeeded",
  failed: "build failed",
  cancelled: "cancelled",
  analysisFailed: "artifact analysis or budget enforcement failed; completed quarantine moves remain undoable",
};

const reasonLabels: Record<ProtectionReason, string> = {
  unowned: "Not created by an approved build",
  nonGeneration: "Not a removable generation",
  active: "Currently active",
  currentProfile: "Current build profile",
  currentTarget: "Current target",
  dependency: "Dependency output",
  incremental: "Incremental build state",
  latestSuccessfulBuild: "Touched by the latest successful build",
  recentExternalChange: "Changed recently outside the coordinator",
  unreadable: "Could not be read safely",
  ambiguous: "Identity or path is ambiguous",
  missingIdentity: "Stable file identity is unavailable",
};

export function BuildArtifactCoordinator() {
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
  const labelErrors = labels.map((value) => value ? textError(value, MAX_STRING_BYTES, true) : undefined);
  const argumentErrors = argv.map((value) => textError(value, MAX_ARGUMENT_BYTES));
  const pathErrors = artifactPathErrors(artifacts.map((artifact) => artifact.relativePath))
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
          <h2 id="build-artifacts-heading">Build artifact budgets</h2>
          <p>Run one approved native build, then quarantine only stale owned generations.</p>
        </div>
        <p>{state.policy?.enabled ? "Automatic budgets enabled" : "Automatic budgets disabled"}</p>
      </header>

      <form className="build-profile-form" onSubmit={(event) => void submit(event)}>
        {rootStatus === "loading" && <p role="status">Loading project roots…</p>}
        {rootError && <>
          <p className="build-artifact-error" role="alert">{rootError}</p>
          <button type="button" className="secondary-button" onClick={() => void reloadRoots()}>Retry loading project roots</button>
        </>}
        {rootStatus === "ready" && roots.length === 0 && <p>No project roots are registered. Add one on Cleanup to register a build profile.</p>}
        <fieldset disabled={Boolean(state.pending) || rootStatus !== "ready"}>
          <legend>Register a build profile</legend>
          <p className="form-help">
            Windows confirms the executable, every argument, working directory, and artifact path.
            Names and labels allow at most {MAX_STRING_BYTES} UTF-8 bytes each.
          </p>
          <div className="build-form-grid">
            <div>
              <label>
                Profile name
                <input required aria-invalid={Boolean(labelErrors[0])} aria-describedby={labelErrors[0] ? "name-error" : undefined} value={displayName} onChange={(event) => setDisplayName(event.target.value)} />
              </label>
              <FieldError id="name-error" error={labelErrors[0]} />
            </div>
            <label>
              Project root
              <select required value={rootId} onChange={(event) => setRootId(event.target.value)}>
                <option value="" disabled>Select a saved root</option>
                {roots.map((root) => <option key={root.id} value={root.id}>{root.displayPath}</option>)}
              </select>
            </label>
            <label>
              Ecosystem
              <select value={ecosystem} onChange={(event) => setEcosystem(event.target.value as BuildEcosystem)}>
                <option value="rust">Rust</option>
                <option value="node">Node</option>
                <option value="generic">Generic wrapper</option>
              </select>
            </label>
            <label className="wide-field">
              Native executable (.exe)
              <input required value={executable} onChange={(event) => setExecutable(event.target.value)} spellCheck={false} />
            </label>
            <label>
              Working directory, relative to root
              <input value={workingDirectory} onChange={(event) => setWorkingDirectory(event.target.value)} placeholder="Leave empty for project root" spellCheck={false} />
            </label>
            <div>
              <label>
                Profile label
                <input required aria-invalid={Boolean(labelErrors[1])} aria-describedby={labelErrors[1] ? "profile-error" : undefined} value={profileLabel} onChange={(event) => setProfileLabel(event.target.value)} />
              </label>
              <FieldError id="profile-error" error={labelErrors[1]} />
            </div>
            <div>
              <label>
                Toolchain label
                <input required aria-invalid={Boolean(labelErrors[2])} aria-describedby={labelErrors[2] ? "toolchain-error" : undefined} value={toolchainLabel} onChange={(event) => setToolchainLabel(event.target.value)} />
              </label>
              <FieldError id="toolchain-error" error={labelErrors[2]} />
            </div>
            <div>
              <label>
                Target label
                <input required aria-invalid={Boolean(labelErrors[3])} aria-describedby={labelErrors[3] ? "target-error" : undefined} value={targetLabel} onChange={(event) => setTargetLabel(event.target.value)} />
              </label>
              <FieldError id="target-error" error={labelErrors[3]} />
            </div>
            <label>
              Rebuild cost
              <select value={rebuildCost} onChange={(event) => setRebuildCost(event.target.value as RebuildCost)}>
                <option value="low">Low</option><option value="medium">Medium</option><option value="high">High</option>
              </select>
            </label>
          </div>

          <fieldset className="repeatable-fields">
            <legend>Arguments, in order</legend>
            <p className="form-help">Each row is one argument. Maximum {MAX_PROFILE_ARGUMENTS} arguments, {MAX_ARGUMENT_BYTES} UTF-8 bytes each. Shell command text is not accepted.</p>
            {argv.map((argument, index) => (
              <div className="repeatable-row" key={`argument-${index}`}>
                <div>
                  <label>
                    Argument {index + 1}
                    <input aria-invalid={Boolean(argumentErrors[index])} aria-describedby={argumentErrors[index] ? `argument-error-${index}` : undefined} value={argument} onChange={(event) => setArgv(argv.map((item, itemIndex) => itemIndex === index ? event.target.value : item))} />
                  </label>
                  <FieldError id={`argument-error-${index}`} error={argumentErrors[index]} />
                </div>
                <button type="button" className="secondary-button" disabled={argv.length === 1} onClick={() => setArgv(argv.filter((_, itemIndex) => itemIndex !== index))}>Remove</button>
              </div>
            ))}
            <button type="button" className="secondary-button" disabled={argv.length >= MAX_PROFILE_ARGUMENTS} onClick={() => setArgv((current) => current.length < MAX_PROFILE_ARGUMENTS ? [...current, ""] : current)}>Add argument</button>
          </fieldset>

          <fieldset className="repeatable-fields">
            <legend>Registered artifact paths</legend>
            <p className="form-help">Use up to {MAX_PROFILE_ARTIFACTS} relative paths, without duplicates or overlaps. Dependencies and incremental state are always protected.</p>
            {artifacts.map((artifact, index) => (
              <div className="repeatable-row artifact-row" key={`artifact-${index}`}>
                <div>
                  <label>
                    Relative path {index + 1}
                    <input required aria-invalid={Boolean(pathErrors[index])} aria-describedby={pathErrors[index] ? `path-error-${index}` : undefined} value={artifact.relativePath} onChange={(event) => setArtifacts(artifacts.map((item, itemIndex) => itemIndex === index ? { ...item, relativePath: event.target.value } : item))} spellCheck={false} />
                  </label>
                  <FieldError id={`path-error-${index}`} error={pathErrors[index]} />
                </div>
                <label>
                  Role
                  <select value={artifact.role} onChange={(event) => setArtifacts(artifacts.map((item, itemIndex) => itemIndex === index ? { ...item, role: event.target.value as ArtifactRole } : item))}>
                    <option value="generation">Generation</option><option value="dependency">Dependency</option><option value="incremental">Incremental</option>
                  </select>
                </label>
                <button type="button" className="secondary-button" disabled={artifacts.length === 1} onClick={() => setArtifacts(artifacts.filter((_, itemIndex) => itemIndex !== index))}>Remove</button>
              </div>
            ))}
            <button type="button" className="secondary-button" disabled={artifacts.length >= MAX_PROFILE_ARTIFACTS} onClick={() => setArtifacts((current) => current.length < MAX_PROFILE_ARTIFACTS ? [...current, { relativePath: "", role: "generation" }] : current)}>Add artifact path</button>
          </fieldset>
          <button type="submit" disabled={invalid}>Review and register profile</button>
        </fieldset>
      </form>

      {state.error && <p className="build-artifact-error" role="alert">{state.error}</p>}
      <p className="build-run-status" role="status" aria-live="polite">
        {state.run ? `Latest build: ${runStateLabels[state.run.state]}${state.run.exitCode != null ? `, exit code ${state.run.exitCode}` : ""}` : "No coordinated build has run in this session."}
      </p>

      <section className="saved-build-profiles" aria-labelledby="saved-build-profiles-heading">
        <h3 id="saved-build-profiles-heading">Saved profiles</h3>
        {state.loading && <p role="status">Loading profiles…</p>}
        {!state.loading && state.profiles.length === 0 && <p>No build profiles are registered.</p>}
        <ul>
          {state.profiles.map((profile) => {
            const running = state.run?.profileId === profile.profileId && !["succeeded", "failed", "cancelled", "analysisFailed"].includes(state.run.state);
            return <li key={profile.profileId}>
              <div>
                <strong>{profile.displayName}</strong><small>{profile.ecosystem} · {profile.profileLabel} · {profile.targetLabel}</small>
                <details className="build-profile-details">
                  <summary aria-label={`Details for ${profile.displayName}`}>Details</summary>
                  <dl>
                    <dt>Executable</dt><dd><code>{profile.executable}</code></dd>
                    <dt>Arguments, in order</dt>
                    <dd>{profile.argv.length === 0 ? "No arguments" : <ol aria-label="Arguments, in order">
                      {profile.argv.map((argument, index) => <li key={index}><code>{argument}</code>{argument === "" && <span>Empty argument</span>}</li>)}
                    </ol>}</dd>
                    <dt>Working directory, relative to root</dt><dd><code>{profile.workingDirectory || "."}</code>{profile.workingDirectory === "" && " (Project root)"}</dd>
                    <dt>Rebuild cost</dt><dd>{profile.rebuildCost}</dd>
                    <dt>Artifact paths, relative to root</dt>
                    <dd><ul aria-label="Artifact paths and roles">
                      {profile.artifactPaths.map((artifact) => <li key={artifact.relativePath}><code>{artifact.relativePath}</code> — <span>{artifact.role}</span></li>)}
                    </ul></dd>
                  </dl>
                </details>
              </div>
              <div className="button-row">
                <button type="button" disabled={Boolean(state.pending) || state.buildActionsDisabled} onClick={() => void state.start(profile.profileId)}>Run</button>
                {running && <button type="button" className="secondary-button" disabled={state.pending === "cancel"} onClick={() => void state.cancel()}>Cancel</button>}
                <button type="button" className="danger-button" disabled={Boolean(state.pending) || state.buildActionsDisabled} aria-describedby={`forget-${profile.profileId}`} onClick={() => void state.remove(profile.profileId)}>Forget</button>
              </div>
              <small id={`forget-${profile.profileId}`}>Forgetting stops future runs and budgets; it does not delete artifacts.</small>
            </li>;
          })}
        </ul>
      </section>

      <section className="budget-preview" aria-labelledby="budget-preview-heading">
        <div className="project-root-manager-heading">
          <h3 id="budget-preview-heading">Budget preview</h3>
          <button type="button" className="secondary-button" disabled={state.loading} onClick={state.reload}>Refresh preview</button>
        </div>
        {state.preview && <>
          <dl className="cleanup-accounting">
            <div><dt>Current</dt><dd>{formatBytes(state.preview.decision.currentAllocatedBytes)}</dd></div>
            <div><dt>Projected</dt><dd>{formatBytes(state.preview.decision.projectedAllocatedBytes)}</dd></div>
            <div><dt>Quarantine</dt><dd>{formatBytes(state.preview.decision.quarantineBytes)}</dd></div>
          </dl>
          {state.preview.decision.unsatisfiedProtectedByteFloor != null && <p className="protected-floor" role="status">Protected data alone uses {formatBytes(state.preview.decision.unsatisfiedProtectedByteFloor)}. The limit remains unmet without deleting protected state.</p>}
          <h4>Per-project preview</h4>
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
              <p>{mode === "explicit" ? "Explicit project limits" : mode === "disabled" ? "Disabled for this project" : "Inherit global limits"}</p>
              {mode !== "disabled" && <p className="form-help">
                Size limit: {maximum == null ? "None" : formatBytes(maximum)} · Age limit: {limits?.maximumAgeSeconds == null ? "None" : `${limits.maximumAgeSeconds / 86_400} days`}
              </p>}
              <dl className="cleanup-accounting">
                <div><dt>Current</dt><dd>{formatBytes(decision.projectCurrentBytes[projectId] ?? 0)}</dd></div>
                <div><dt>Projected</dt><dd>{formatBytes(decision.projectProjectedBytes[projectId] ?? 0)}</dd></div>
                <div><dt>Protected floor</dt><dd>{formatBytes(floor)}</dd></div>
              </dl>
              {maximum != null && floor > maximum && <p className="protected-floor" role="status">Protected data exceeds this project’s {formatBytes(maximum)} size limit. The limit cannot be met without deleting protected state.</p>}
            </section>;
          })}
          <h4>Selected generations</h4>
          {state.preview.decision.selectedGenerationIds.length === 0 ? <p>Nothing is selected for quarantine.</p> : <ul>{state.preview.decision.selectedGenerationIds.map((id) => <li key={id}><strong>{generations.get(id)?.normalizedPath ?? id}</strong><span>{formatBytes(generations.get(id)?.allocatedBytes ?? 0)}</span></li>)}</ul>}
          <h4>Protected generations</h4>
          {state.preview.decision.protected.length === 0 ? <p>No protected generations are recorded.</p> : <ul>{state.preview.decision.protected.map((item) => <li key={item.generationId}><strong>{generations.get(item.generationId)?.normalizedPath ?? item.generationId}</strong><span>{item.reasons.map((reason) => reasonLabels[reason]).join(", ")}</span></li>)}</ul>}
        </>}
      </section>
    </section>
  );
}
