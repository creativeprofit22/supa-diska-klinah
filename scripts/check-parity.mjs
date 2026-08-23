import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const parityPath = resolve(import.meta.dirname, "..", "docs", "parity.md");
const document = readFileSync(parityPath, "utf8");
const revision = "db09e051d0615121e659db187e3799438acbc9e6";
const modules = [
  "Cleaner", "Browser", "LargeFiles", "Duplicates", "Memory", "Startup",
  "Registry", "Uninstaller", "Drivers", "Network", "DiskHealth", "StorageSense",
  "Battery", "Debloater", "Privacy", "Optimizer", "System", "Telemetry",
  "Notifications", "PowerPlan", "Hosts", "Restore", "Environment", "Repair",
  "Scheduler", "Updates", "Firewall", "ContextMenu", "Gpu", "BootTrace",
  "RegistryBackup", "CloudCleanup", "FileShredder", "GameMode",
];
const directGroups = [
  "Cleaner location and blockers",
  "Platform information",
  "Settings and backup directory",
  "Onboarding",
  "Elevation",
  "Restore points",
  "Scan, deletion, and cloud history",
  "Updater operations",
];
const implementationStatuses = new Set(["Contract mapped", "Implemented"]);
const verificationStatuses = new Set(["Not verified", "Verified"]);

function fail(message) {
  console.error(`Parity check failed: ${message}`);
  process.exit(1);
}

function rowsAfter(header) {
  const lines = document.split(/\r?\n/);
  const headerIndex = lines.indexOf(header);
  if (headerIndex < 0 || !/^\|(?:\s*:?-+:?\s*\|)+$/.test(lines[headerIndex + 1])) {
    fail(`missing required table header: ${header}`);
  }

  const rows = [];
  for (const line of lines.slice(headerIndex + 2)) {
    if (!line.startsWith("|")) break;
    rows.push(line.slice(1, -1).split("|").map((cell) => cell.trim()));
  }
  return rows;
}

if (!document.includes(`Kudu v2.4.0`) || !document.includes(revision)) {
  fail("source version and immutable revision must be cited");
}

const moduleRows = rowsAfter(
  "| Kudu module | Kudu source module | Target Tauri command module | Target Rust crate/module | Target frontend feature | Implementation status | Verification status |",
);
const directRows = rowsAfter(
  "| Direct handler group | Kudu source | Target Tauri command module | Target Rust crate/module | Target frontend feature | Implementation status | Verification status |",
);

function verifyInventory(rows, expected, label) {
  if (rows.some((row) => row.length !== 7 || row.some((cell) => !cell))) {
    fail(`${label} rows must contain all seven required columns`);
  }
  const names = rows.map(([name]) => name);
  if (new Set(names).size !== names.length) fail(`${label} rows must be unique`);
  const missing = expected.filter((name) => !names.includes(name));
  const unexpected = names.filter((name) => !expected.includes(name));
  if (missing.length || unexpected.length || names.length !== expected.length) {
    fail(`${label} inventory mismatch; missing: ${missing.join(", ") || "none"}; unexpected: ${unexpected.join(", ") || "none"}`);
  }
  for (const row of rows) {
    if (!implementationStatuses.has(row[5])) fail(`invalid implementation status: ${row[5]}`);
    if (!verificationStatuses.has(row[6])) fail(`invalid verification status: ${row[6]}`);
  }
}

verifyInventory(moduleRows, modules, "registered module");
verifyInventory(directRows, directGroups, "direct handler");
console.log("Kudu parity contract verified.");
