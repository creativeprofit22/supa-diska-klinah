import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { realpathSync } from "node:fs";

// Exact step names from .github/workflows/ci.yml. Renaming a step there without
// renaming it here is the drift this checker exists to catch.
export const NATIVE_SMOKE_STEPS = [
  "Build native debug executable",
  "Verify native project discovery and storage workflows as standard user",
];
// The renderer replay only runs where the runner image ships the browser it uses.
export const RENDERER_REPLAY_STEP = "Replay actual storage routes in the preview browser";
export const RENDERER_REPLAY_ARCHITECTURES = ["x64"];
export const NATIVE_ARCHITECTURES = ["x64", "ARM64"];

export function requiredSteps(architecture) {
  return RENDERER_REPLAY_ARCHITECTURES.includes(architecture)
    ? [...NATIVE_SMOKE_STEPS, RENDERER_REPLAY_STEP]
    : [...NATIVE_SMOKE_STEPS];
}

/** Returns every reason the run does not gate the given revision; empty means verified. */
export function collectFailures(run, headSha) {
  const failures = [];
  if (!run || typeof run !== "object") return ["workflow run payload could not be read"];
  if (run.headSha !== headSha) failures.push("workflow run is not for the current revision");
  if (run.status !== "completed" || run.conclusion !== "success") {
    failures.push(`workflow run ended with ${run.status}/${run.conclusion}`);
  }
  const jobs = Array.isArray(run.jobs) ? run.jobs : [];
  for (const architecture of NATIVE_ARCHITECTURES) {
    const job = jobs.find(({ name }) => name === `Native smoke (${architecture})`);
    if (!job || job.conclusion !== "success") {
      failures.push(`${architecture} native job did not pass`);
      continue;
    }
    const steps = Array.isArray(job.steps) ? job.steps : [];
    for (const stepName of requiredSteps(architecture)) {
      const step = steps.find(({ name }) => name === stepName);
      if (!step) failures.push(`${architecture} ${stepName} is missing`);
      else if (step.conclusion !== "success") {
        failures.push(`${architecture} ${stepName} did not pass`);
      }
    }
  }
  return failures;
}

function fail(message) {
  console.error(`Native CI check failed: ${message}`);
  process.exit(1);
}

function main() {
  const runId = process.argv[2];
  if (!/^\d+$/.test(runId ?? "")) fail("provide a numeric GitHub Actions run ID");

  const head = spawnSync("git", ["rev-parse", "HEAD"], {
    encoding: "utf8",
    shell: false,
  });
  if (head.status !== 0) fail(head.stderr.trim() || "could not read the current revision");

  const result = spawnSync(
    "gh",
    ["run", "view", runId, "--json", "headSha,status,conclusion,jobs"],
    { encoding: "utf8", maxBuffer: 4 * 1024 * 1024, shell: false },
  );
  if (result.status !== 0) fail(result.stderr.trim() || "could not read the workflow run");

  const failures = collectFailures(JSON.parse(result.stdout), head.stdout.trim());
  if (failures.length > 0) fail(failures.join("; "));

  console.log(
    "Current-revision x64 and ARM64 native CI verified, including the standard-user storage smoke.",
  );
}

const entry = process.argv[1];
if (entry && realpathSync(entry) === realpathSync(fileURLToPath(import.meta.url))) main();
