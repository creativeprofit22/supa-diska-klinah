import { existsSync, readFileSync } from "node:fs";

function fail(message) {
  console.error(`Dependency check failed: ${message}`);
  process.exit(1);
}

const packageJson = JSON.parse(readFileSync("package.json", "utf8"));
const dependencies = { ...packageJson.dependencies, ...packageJson.devDependencies };
const inexactDependency = Object.entries(dependencies).find(
  ([, version]) => !/^\d+\.\d+\.\d+$/.test(version),
);

if (inexactDependency) fail(`${inexactDependency[0]} is not pinned exactly`);
if (!/^pnpm@11\.22\.0\+sha512\.[0-9a-f]{128}$/.test(packageJson.packageManager)) {
  fail("packageManager must pin pnpm 11.22.0 with Corepack integrity");
}
if (readFileSync(".node-version", "utf8").trim() !== "24.19.0") {
  fail("Node must remain pinned to 24.19.0");
}

const rustToolchain = readFileSync("src-tauri/rust-toolchain.toml", "utf8");
for (const required of [
  'channel = "1.90.0"',
  '"x86_64-pc-windows-msvc"',
  '"aarch64-pc-windows-msvc"',
]) {
  if (!rustToolchain.includes(required)) fail(`missing Rust toolchain pin: ${required}`);
}

const cargo = readFileSync("src-tauri/Cargo.toml", "utf8");
for (const required of ['version = "=2.11.5"', 'version = "=2.6.3"']) {
  if (!cargo.includes(required)) fail(`missing Cargo pin: ${required}`);
}
const cleanupCoreCargo = readFileSync("src-tauri/crates/cleanup-core/Cargo.toml", "utf8");
if (!cleanupCoreCargo.includes('version = "=1.0.229"')) {
  fail("Serde must remain pinned to 1.0.229");
}
if (!cleanupCoreCargo.includes('serde_json = "=1.0.151"')) {
  fail("cleanup-core serde_json must remain pinned to 1.0.151");
}

const workflow = readFileSync(".github/workflows/ci.yml", "utf8");
const actionLines = workflow.split(/\r?\n/).filter((line) => line.trim().startsWith("uses:"));
if (
  actionLines.length === 0 ||
  actionLines.some((line) => !/@[0-9a-f]{40}$/.test(line.trim()))
) {
  fail("every GitHub Action must use a full immutable commit SHA");
}

for (const lockfile of ["pnpm-lock.yaml", "src-tauri/Cargo.lock"]) {
  if (!existsSync(lockfile)) fail(`missing lockfile: ${lockfile}`);
}

console.log("Dependency pins and lockfiles verified.");
