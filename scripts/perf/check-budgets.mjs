// Compares a perf results.json against scripts/perf/budgets.json.
// Machine-speed budgets apply only when the run's CPU matches the budget's machine class;
// otherwise they are reported as skipped (never silently passed as met).
// Usage: node scripts/perf/check-budgets.mjs <results.json> [...more] [--budgets <file>]
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

/**
 * @param {unknown} value
 * @param {readonly string[]} keys
 * @returns {unknown}
 */
export function lookup(value, keys) {
  let current = value;
  for (const key of keys) {
    if (current === null || typeof current !== "object" || !Object.hasOwn(current, key)) return undefined;
    current = /** @type {Record<string, unknown>} */ (current)[key];
  }
  return current;
}

/**
 * @typedef {{ id: string, kind: string, path: string[], max?: number, min?: number, machineIndependent?: boolean }} Budget
 * @typedef {{ schemaVersion: 1, machineClass: { cpuModel: string, logicalProcessors: number }, budgets: Budget[] }} BudgetFile
 * @typedef {{ id: string, status: "pass" | "fail" | "skipped" | "missing" | "invalid", actual?: number, limit?: string, reason?: string }} Outcome
 */

/** @param {unknown} file @returns {BudgetFile} */
export function validateBudgetFile(file) {
  const fail = (message) => {
    throw new Error(`Invalid budgets file: ${message}`);
  };
  if (lookup(file, ["schemaVersion"]) !== 1) fail("schemaVersion must be 1");
  const machine = lookup(file, ["machineClass"]);
  if (typeof lookup(machine, ["cpuModel"]) !== "string") fail("machineClass.cpuModel must be a string");
  if (!Number.isInteger(lookup(machine, ["logicalProcessors"]))) fail("machineClass.logicalProcessors must be an integer");
  const budgets = lookup(file, ["budgets"]);
  if (!Array.isArray(budgets) || budgets.length === 0) fail("budgets must be a non-empty array");
  const ids = new Set();
  for (const budget of /** @type {unknown[]} */ (budgets)) {
    const id = lookup(budget, ["id"]);
    if (typeof id !== "string" || !/^[a-z0-9.-]+$/.test(id)) fail(`bad budget id ${String(id)}`);
    if (ids.has(id)) fail(`duplicate budget id ${id}`);
    ids.add(id);
    if (typeof lookup(budget, ["kind"]) !== "string") fail(`${id}: kind must be a string`);
    const keys = lookup(budget, ["path"]);
    if (!Array.isArray(keys) || keys.length === 0 || !keys.every((k) => typeof k === "string")) fail(`${id}: path must be string[]`);
    const max = lookup(budget, ["max"]);
    const min = lookup(budget, ["min"]);
    if (max === undefined && min === undefined) fail(`${id}: needs max or min`);
    for (const bound of [max, min]) {
      if (bound !== undefined && (typeof bound !== "number" || !Number.isFinite(bound))) fail(`${id}: bounds must be finite numbers`);
    }
  }
  return /** @type {BudgetFile} */ (file);
}

/**
 * @param {BudgetFile} budgetFile
 * @param {unknown} results parsed results.json
 * @returns {Outcome[]}
 */
export function evaluate(budgetFile, results) {
  const kind = lookup(results, ["kind"]);
  const cpu = lookup(results, ["environment", "cpu", "model"]);
  const logical = lookup(results, ["environment", "cpu", "logicalProcessors"]);
  const sameMachine = cpu === budgetFile.machineClass.cpuModel && logical === budgetFile.machineClass.logicalProcessors;
  const sleepEvents = lookup(results, ["sleepEvents"]);
  const sleptDuringRun = Array.isArray(sleepEvents) && sleepEvents.length > 0;
  return budgetFile.budgets
    .filter((budget) => budget.kind === kind)
    .map((budget) => {
      const limit = [budget.min !== undefined ? `>= ${budget.min}` : "", budget.max !== undefined ? `<= ${budget.max}` : ""]
        .filter(Boolean)
        .join(" and ");
      if (!budget.machineIndependent && !sameMachine) {
        return { id: budget.id, status: "skipped", limit, reason: `machine class differs (${String(cpu)})` };
      }
      if (!budget.machineIndependent && sleptDuringRun) {
        return { id: budget.id, status: "invalid", limit, reason: "the machine slept during the run; rerun it" };
      }
      const actual = lookup(results, budget.path);
      if (typeof actual !== "number" || !Number.isFinite(actual)) {
        return { id: budget.id, status: "missing", limit, reason: `no numeric value at ${budget.path.join(" > ")}` };
      }
      const ok = (budget.max === undefined || actual <= budget.max) && (budget.min === undefined || actual >= budget.min);
      return { id: budget.id, status: ok ? "pass" : "fail", actual, limit };
    });
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const budgetIndex = args.indexOf("--budgets");
  const budgetPath =
    budgetIndex >= 0 ? args.splice(budgetIndex, 2)[1] : path.join(path.dirname(fileURLToPath(import.meta.url)), "budgets.json");
  if (budgetPath === undefined || args.length === 0) {
    console.error("Usage: node scripts/perf/check-budgets.mjs <results.json> [...] [--budgets <file>]");
    process.exit(2);
  }
  const budgets = validateBudgetFile(JSON.parse(await readFile(budgetPath, "utf8")));
  let failed = false;
  for (const file of args) {
    const results = JSON.parse((await readFile(file, "utf8")).replace(/^\uFEFF/, ""));
    const outcomes = evaluate(budgets, results);
    console.log(`${file} (${String(lookup(results, ["kind"]))})`);
    if (outcomes.length === 0) console.log("  no budgets for this kind");
    for (const outcome of outcomes) {
      const detail = outcome.actual !== undefined ? `${outcome.actual} (${outcome.limit})` : `${outcome.limit}: ${outcome.reason}`;
      console.log(`  ${outcome.status.toUpperCase().padEnd(7)} ${outcome.id}: ${detail}`);
      if (outcome.status === "fail" || outcome.status === "missing" || outcome.status === "invalid") failed = true;
    }
  }
  process.exitCode = failed ? 1 : 0;
}
