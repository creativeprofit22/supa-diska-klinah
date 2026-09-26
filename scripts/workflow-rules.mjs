// Minimal, dependency-free reader for the GitHub workflow shapes this repo uses:
// top-level `jobs:` with two-space job keys, four-space job properties and
// `- name:` steps at six spaces. Enough to enforce hygiene rules without a YAML
// dependency; the rules fail closed if the layout is not recognised.

/** Splits a workflow into `{ name, body }` jobs. */
export function jobs(workflow) {
  const lines = workflow.split(/\r?\n/);
  const start = lines.findIndex((line) => line === "jobs:");
  if (start < 0) return [];
  const result = [];
  let current = null;
  for (const line of lines.slice(start + 1)) {
    const key = /^ {2}([A-Za-z0-9_-]+):\s*$/.exec(line);
    if (key) {
      current = { name: key[1], lines: [] };
      result.push(current);
    } else if (/^\S/.test(line)) {
      break;
    } else if (current) {
      current.lines.push(line);
    }
  }
  return result.map(({ name, lines: body }) => ({ name, body: body.join("\n") }));
}

/** Steps of one job body, each as its raw text block. */
export function steps(jobBody) {
  const blocks = [];
  let current = null;
  for (const line of jobBody.split("\n")) {
    if (/^ {6}- /.test(line)) {
      current = [line];
      blocks.push(current);
    } else if (current && (/^ {8}/.test(line) || line.trim() === "")) {
      current.push(line);
    } else if (current && /^ {0,5}\S/.test(line)) {
      current = null;
    }
  }
  return blocks.map((block) => block.join("\n"));
}

export function stepName(step) {
  return /- name: (.+)/.exec(step)?.[1]?.trim() ?? "";
}

/** Job-level (four-space) `permissions:` block, or null. */
export function jobPermissions(jobBody) {
  const lines = jobBody.split("\n");
  const start = lines.findIndex((line) => /^ {4}permissions:/.test(line));
  if (start < 0) return null;
  const scalar = /^ {4}permissions:\s*(\S.*)$/.exec(lines[start])?.[1];
  if (scalar) return { _scalar: scalar.trim() };
  const permissions = {};
  for (const line of lines.slice(start + 1)) {
    const entry = /^ {6}([A-Za-z-]+):\s*(\S+)\s*$/.exec(line);
    if (!entry) break;
    permissions[entry[1]] = entry[2];
  }
  return permissions;
}

const RELEASE_BUILD = "windows-release-build";
const RELEASE_PUBLISH = "windows-release-publish";
const SIGNING_SECRETS = /secrets\.WINDOWS_CODESIGN_PFX_(?:BASE64|PASSWORD)/;

/**
 * Release pipeline rules (fail closed, least privilege, signing dormant until
 * `authenticode`). Returns failure messages; empty means compliant.
 */
export function releaseFailures(workflow, scripts) {
  const failures = [];
  const byName = new Map(jobs(workflow).map((job) => [job.name, job.body]));
  const build = byName.get(RELEASE_BUILD);
  const publish = byName.get(RELEASE_PUBLISH);
  if (!build || !publish) return [`release must be split into ${RELEASE_BUILD} and ${RELEASE_PUBLISH} jobs`];
  if (byName.has("windows-release")) failures.push("the old single windows-release job must be removed");

  const buildPermissions = jobPermissions(build) ?? {};
  const publishPermissions = jobPermissions(publish) ?? {};
  if (JSON.stringify(buildPermissions) !== JSON.stringify({ contents: "read", "id-token": "write", attestations: "write" })) {
    failures.push("release build permissions must be exactly contents: read, id-token: write, attestations: write");
  }
  if (JSON.stringify(publishPermissions) !== JSON.stringify({ contents: "write" })) {
    failures.push("release publish permissions must be exactly contents: write");
  }
  for (const [name, body] of [[RELEASE_BUILD, build], [RELEASE_PUBLISH, publish]]) {
    if (!/^ {4}environment: windows-release\s*$/m.test(body)) failures.push(`${name} must run in the windows-release environment`);
    if (/continue-on-error/.test(body)) failures.push(`${name} must not use continue-on-error`);
  }
  if (!/^ {4}needs: \[windows-release-build\]\s*$/m.test(publish)) failures.push("publish must need the release build");
  if (!/^ {4}if: startsWith\(github\.ref, 'refs\/tags\/v'\)\s*$/m.test(publish)) failures.push("publish must run for v* tags only");
  if (!/^ {4}needs: \[quality, native-smoke, dependency-audit\]\s*$/m.test(build)) {
    failures.push("release build must need quality, native-smoke and dependency-audit");
  }

  const buildSteps = steps(build);
  const names = buildSteps.map(stepName);
  const modeIndex = names.indexOf("Validate declared signing mode");
  const firstBuild = buildSteps.findIndex((step) => /tauri build|pnpm install/.test(step));
  if (modeIndex < 0 || !/check-signing-mode\.ps1 -Mode \$env:SIGNING_MODE/.test(buildSteps[modeIndex] ?? "")) {
    failures.push("release build must validate the signing mode with check-signing-mode.ps1");
  } else if (firstBuild >= 0 && modeIndex > firstBuild) {
    failures.push("the signing mode must be validated before any install or build step");
  }
  if (!/^ {6}SIGNING_MODE: \$\{\{ vars\.WINDOWS_SIGNING_MODE \}\}\s*$/m.test(build)) {
    failures.push("release build must take SIGNING_MODE from vars.WINDOWS_SIGNING_MODE");
  }
  for (const step of buildSteps) {
    const guarded = /^ {8}if: env\.SIGNING_MODE == 'authenticode'\s*$/m.test(step) ||
      /^ {8}if: always\(\) && env\.SIGNING_MODE == 'authenticode'\s*$/m.test(step);
    if ((SIGNING_SECRETS.test(step) || /prepare-windows-signing\.ps1|TAURI_RELEASE_CONFIG/.test(step)) && !guarded) {
      failures.push(`step "${stepName(step)}" uses signing secrets or config outside an authenticode guard`);
    }
  }
  if (SIGNING_SECRETS.test(publish)) failures.push("publish must not see code-signing secrets");
  if (/secrets\.UPDATE_SIGNING_KEY/.test(publish)) failures.push("publish must not see the update signing key");
  const keySteps = buildSteps.filter((step) => /secrets\.UPDATE_SIGNING_KEY/.test(step));
  if (keySteps.length !== 1 || stepName(keySteps[0]) !== "Stage release assets") {
    failures.push("the update signing key may be used only by the Stage release assets step");
  }
  const workflowKeyUses = workflow.match(/secrets\.UPDATE_SIGNING_KEY/g) ?? [];
  if (workflowKeyUses.length !== 1) failures.push("the update signing key must be referenced exactly once in the workflow");

  const verifyIndex = names.indexOf("Verify installed release in the declared signing mode");
  const stageIndex = names.indexOf("Stage release assets");
  const attestIndex = buildSteps.findIndex((step) => /actions\/attest-build-provenance@/.test(step));
  const uploadIndex = buildSteps.findIndex((step) => /actions\/upload-artifact@/.test(step));
  if (!(verifyIndex >= 0 && verifyIndex < stageIndex && stageIndex < attestIndex && attestIndex < uploadIndex)) {
    failures.push("release build must verify, then stage, then attest, then upload");
  }
  if (!/verify-windows-release\.ps1 -SigningMode unsigned/.test(build) || !/verify-windows-release\.ps1 -SigningMode authenticode -ExpectedThumbprint/.test(build)) {
    failures.push("release verification must run in the declared mode with an expected signer when signed");
  }
  if (/--debug|--no-bundle/.test(build)) failures.push("release builds must be optimised NSIS bundles");

  const publishSteps = steps(publish);
  const publishNames = publishSteps.map(stepName);
  const order = ["Download verified release assets", "Re-verify release assets", "Verify build provenance attestations", "Create GitHub release", "Publish update manifest"]
    .map((name) => publishNames.indexOf(name));
  if (order.some((index) => index < 0) || order.some((index, i) => i > 0 && index <= order[i - 1])) {
    failures.push("publish must download, re-verify, verify attestations, create the release, then publish the manifest last");
  }
  if (!/gh attestation verify/.test(publish)) failures.push("publish must verify provenance attestations");
  // Matched by what runs, not by step name: the manifest push must be the last step and happen once.
  const manifestSteps = publishSteps
    .map((step, index) => [step, index])
    .filter(([step]) => /publish-update-manifest\.mjs|updates branch|contents\/update\.json/.test(step));
  if (manifestSteps.length !== 1 || manifestSteps[0][1] !== publishSteps.length - 1) {
    failures.push("the update manifest must be published exactly once, as the final publish step");
  }
  const releaseIndex = publishSteps.findIndex((step) => /publish-release\.mjs|gh release/.test(step));
  if (releaseIndex >= 0 && releaseIndex < order[2]) failures.push("no release may be created before verification");

  if (!/-SigningMode/.test(scripts.verify) || !/NotSigned/.test(scripts.verify) || !/TimeStamperCertificate/.test(scripts.verify)) {
    failures.push("verify-windows-release.ps1 must enforce the declared signing mode");
  }
  if (!/SigningMode -ne "authenticode"/.test(scripts.prepare)) {
    failures.push("prepare-windows-signing.ps1 must refuse to run outside authenticode mode");
  }
  return failures;
}

/** Hygiene failures for one workflow file. */
export function hygieneFailures(name, workflow) {
  const failures = [];
  const all = jobs(workflow);
  if (all.length === 0) failures.push(`${name}: no jobs found`);
  if (!/^permissions:\s*\n {2}contents: read\s*$/m.test(workflow)) {
    failures.push(`${name}: top-level permissions must be exactly contents: read`);
  }
  for (const job of all) {
    if (!jobPermissions(job.body)) failures.push(`${name}: job ${job.name} must declare its own permissions`);
    for (const step of steps(job.body)) {
      if (/uses: actions\/checkout@/.test(step) && !/persist-credentials: false/.test(step)) {
        failures.push(`${name}: checkout in job ${job.name} must set persist-credentials: false`);
      }
    }
  }
  return failures;
}
