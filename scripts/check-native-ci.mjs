import { spawnSync } from "node:child_process";

function fail(message) {
  console.error(`Native CI check failed: ${message}`);
  process.exit(1);
}

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

const run = JSON.parse(result.stdout);
if (run.headSha !== head.stdout.trim()) fail("workflow run is not for the current revision");
if (run.status !== "completed" || run.conclusion !== "success") {
  fail(`workflow run ended with ${run.status}/${run.conclusion}`);
}

for (const architecture of ["x64", "ARM64"]) {
  const job = run.jobs.find(({ name }) => name === `Native smoke (${architecture})`);
  if (!job || job.conclusion !== "success") fail(`${architecture} native job did not pass`);
  for (const stepName of [
    "Build native debug executable",
    "Launch native executable as standard user",
  ]) {
    const step = job.steps.find(({ name }) => name === stepName);
    if (!step || step.conclusion !== "success") fail(`${architecture} ${stepName} did not pass`);
  }
}

console.log("Current-revision x64 and ARM64 native CI verified.");
