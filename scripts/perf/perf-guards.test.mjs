import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { evaluate, lookup, validateBudgetFile } from "./check-budgets.mjs";
import { checkCargoToml, checkRepository, checkScript } from "./check-build-config.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

test("dev profiles must keep incremental builds", () => {
  const cases = [
    { toml: "[profile.dev]\nincremental = false\n", violations: 1 },
    { toml: "[profile.dev.package.\"*\"]\nincremental = false\n", violations: 1 },
    { toml: "[profile.test]\nincremental=false # speed\n", violations: 1 },
    { toml: "[build]\nincremental = false\n", violations: 1 },
    { toml: "[profile.release]\nincremental = false\n", violations: 0 },
    { toml: "[profile.dev]\n# incremental = false\nopt-level = 0\n", violations: 0 },
    { toml: "[profile.dev.package.sha2]\nopt-level = 3\n", violations: 0 },
  ];
  for (const { toml, violations } of cases) {
    assert.equal(checkCargoToml("Cargo.toml", toml).length, violations, toml);
  }
});

test("committed Cargo config may not force sccache or a wrapper on local builds", () => {
  assert.equal(checkCargoToml(".cargo/config.toml", "[build]\nrustc-wrapper = \"sccache\"\n").length, 1);
  assert.equal(checkCargoToml(".cargo/config.toml", "[env]\nRUSTC_WRAPPER = \"sccache\"\n").length, 1);
  assert.equal(checkCargoToml(".cargo/config.toml", "[env]\nCARGO_INCREMENTAL = \"0\"\n").length, 1);
  assert.equal(checkCargoToml(".cargo/config.toml", "[target.x86_64-pc-windows-msvc]\nlinker = \"rust-lld\"\n").length, 0);
});

test("scripts may not run cargo clean automatically", () => {
  const flagged = [
    "cargo clean",
    "& cargo clean -p app",
    'spawn("cargo", ["clean"])',
    'Invoke-PerfNative -FilePath "cargo" -Arguments @("clean")',
    'Start-Process cargo -ArgumentList "clean"',
  ];
  for (const line of flagged) assert.equal(checkScript("s.ps1", line).length, 1, line);
  const allowed = [
    "# never run cargo clean",
    "// cargo clean is forbidden",
    "<#\n  `cargo clean` is never run\n#>",
    "/* cargo clean */",
    'cargo build --workspace',
    'Write-Output "a clean build"',
    '  "automatic `cargo clean`",',
  ];
  for (const text of allowed) assert.equal(checkScript("s.ps1", text).length, 0, text);
});

test("the repository passes the build configuration guard", async () => {
  assert.deepEqual(await checkRepository(root), []);
});

const budgetFile = validateBudgetFile({
  schemaVersion: 1,
  machineClass: { cpuModel: "Test CPU", logicalProcessors: 8 },
  budgets: [
    { id: "compile.noop", kind: "compile", path: ["results", "noop.rust", "stats", "median"], max: 5000 },
    { id: "runtime.leak", kind: "runtime", path: ["results", "x", "handleLeak"], max: 4, machineIndependent: true },
  ],
});

function compileResults(median, cpu = "Test CPU") {
  return {
    kind: "compile",
    environment: { cpu: { model: cpu, logicalProcessors: 8 } },
    results: { "noop.rust": { stats: { median } } },
  };
}

test("budgets pass, fail, skip on other machines, and report missing metrics", () => {
  const table = [
    { results: compileResults(4000), status: "pass" },
    { results: compileResults(6000), status: "fail" },
    { results: compileResults(6000, "Other CPU"), status: "skipped" },
    { results: { ...compileResults(1), results: {} }, status: "missing" },
    { results: { ...compileResults(4000), sleepEvents: [{ id: 42 }] }, status: "invalid" },
  ];
  for (const { results, status } of table) {
    const [outcome] = evaluate(budgetFile, results);
    assert.equal(outcome?.status, status);
  }
});

test("machine-independent budgets apply on any machine and only to their kind", () => {
  const results = { kind: "runtime", environment: { cpu: { model: "CI", logicalProcessors: 2 } }, results: { x: { handleLeak: 9 } } };
  assert.deepEqual(
    evaluate(budgetFile, results).map((o) => o.status),
    ["fail"],
  );
});

test("lookup never walks into prototypes or missing keys", () => {
  assert.equal(lookup({ a: { b: 1 } }, ["a", "b"]), 1);
  assert.equal(lookup({ a: {} }, ["a", "toString"]), undefined);
  assert.equal(lookup(null, ["a"]), undefined);
});

test("budget files are validated before use", () => {
  assert.throws(() => validateBudgetFile({ schemaVersion: 2 }), /schemaVersion/);
  assert.throws(
    () =>
      validateBudgetFile({
        schemaVersion: 1,
        machineClass: { cpuModel: "x", logicalProcessors: 1 },
        budgets: [{ id: "a", kind: "compile", path: ["x"] }],
      }),
    /needs max or min/,
  );
});

test("the committed budgets file is valid", async () => {
  validateBudgetFile(JSON.parse(await readFile(path.join(root, "scripts/perf/budgets.json"), "utf8")));
});
