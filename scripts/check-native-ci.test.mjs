import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import {
  NATIVE_SMOKE_STEPS,
  RENDERER_REPLAY_STEP,
  collectFailures,
  requiredSteps,
} from "./check-native-ci.mjs";

const REVISION = "1090c0dbe15584b6baef462a3d658ddc346427f9";

function step(name, conclusion = "success") {
  return { name, status: "completed", conclusion };
}

/** Mirrors the shape `gh run view --json headSha,status,conclusion,jobs` returns. */
function representativeRun(overrides = {}) {
  const nativeJob = (architecture) => ({
    name: `Native smoke (${architecture})`,
    status: "completed",
    conclusion: "success",
    steps: [
      step("Set up job"),
      step("Check out repository"),
      step("Set up Node.js"),
      step("Enable Corepack"),
      step("Install frontend dependencies"),
      step("Install pinned Rust toolchain"),
      step("Build native debug executable"),
      step("Verify native project discovery and storage workflows as standard user"),
      ...(architecture === "x64" ? [step("Replay actual storage routes in the preview browser")] : []),
      step("Upload native smoke evidence"),
    ],
  });
  return {
    headSha: REVISION,
    status: "completed",
    conclusion: "success",
    jobs: [
      { name: "Quality", status: "completed", conclusion: "success", steps: [step("Test frontend")] },
      nativeJob("x64"),
      nativeJob("ARM64"),
    ],
    ...overrides,
  };
}

function withNativeSteps(architecture, mutate) {
  const run = representativeRun();
  const job = run.jobs.find(({ name }) => name === `Native smoke (${architecture})`);
  job.steps = mutate(job.steps);
  return run;
}

test("the actual successful workflow structure is accepted", () => {
  assert.deepEqual(collectFailures(representativeRun(), REVISION), []);
});

test("every required step name exists in the checked-in workflow", async () => {
  const workflow = await readFile(".github/workflows/ci.yml", "utf8");
  const declared = new Set(
    [...workflow.matchAll(/^\s*- name: (.+)$/gm)].map((match) => match[1].trim()),
  );
  for (const name of [...NATIVE_SMOKE_STEPS, RENDERER_REPLAY_STEP]) {
    assert.ok(declared.has(name), `workflow is missing the step named: ${name}`);
  }
  assert.match(workflow, /smoke-native-ci\.ps1 -Target \$\{\{ matrix\.target \}\} -StorageSmoke/);
  assert.match(workflow, /run: pnpm check:ports/);
});

test("the renderer replay is required only where the browser is installed", () => {
  assert.deepEqual(requiredSteps("x64"), [...NATIVE_SMOKE_STEPS, RENDERER_REPLAY_STEP]);
  assert.deepEqual(requiredSteps("ARM64"), NATIVE_SMOKE_STEPS);
});

test("a missing storage smoke step is rejected on either architecture", () => {
  for (const architecture of ["x64", "ARM64"]) {
    const run = withNativeSteps(architecture, (steps) =>
      steps.filter(({ name }) => name !== NATIVE_SMOKE_STEPS[1]),
    );
    assert.deepEqual(collectFailures(run, REVISION), [
      `${architecture} ${NATIVE_SMOKE_STEPS[1]} is missing`,
    ]);
  }
});

test("a failed storage smoke step is rejected even when the job reports success", () => {
  const run = withNativeSteps("ARM64", (steps) =>
    steps.map((current) =>
      current.name === NATIVE_SMOKE_STEPS[1] ? step(current.name, "failure") : current,
    ),
  );
  assert.deepEqual(collectFailures(run, REVISION), [`ARM64 ${NATIVE_SMOKE_STEPS[1]} did not pass`]);
});

test("a skipped renderer replay on x64 is rejected", () => {
  const run = withNativeSteps("x64", (steps) =>
    steps.map((current) =>
      current.name === RENDERER_REPLAY_STEP ? step(current.name, "skipped") : current,
    ),
  );
  assert.deepEqual(collectFailures(run, REVISION), [`x64 ${RENDERER_REPLAY_STEP} did not pass`]);
});

test("the old step name alone no longer satisfies the native gate", () => {
  const run = withNativeSteps("x64", (steps) =>
    steps.map((current) =>
      current.name === NATIVE_SMOKE_STEPS[1]
        ? step("Launch native executable as standard user")
        : current,
    ),
  );
  assert.deepEqual(collectFailures(run, REVISION), [`x64 ${NATIVE_SMOKE_STEPS[1]} is missing`]);
});

test("a run for another revision is rejected", () => {
  const run = representativeRun({ headSha: "0277e4990d09082d768f295d8ef151bac26152f3" });
  assert.deepEqual(collectFailures(run, REVISION), [
    "workflow run is not for the current revision",
  ]);
});

test("an unfinished or failed run is rejected", () => {
  assert.deepEqual(collectFailures(representativeRun({ status: "in_progress", conclusion: null }), REVISION), [
    "workflow run ended with in_progress/null",
  ]);
  const failed = representativeRun({ conclusion: "failure" });
  assert.ok(collectFailures(failed, REVISION).includes("workflow run ended with completed/failure"));
});

test("a missing native job is rejected", () => {
  const run = representativeRun();
  run.jobs = run.jobs.filter(({ name }) => name !== "Native smoke (ARM64)");
  assert.deepEqual(collectFailures(run, REVISION), ["ARM64 native job did not pass"]);
});
