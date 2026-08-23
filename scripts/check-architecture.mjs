import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, extname, relative, resolve, sep } from "node:path";
import { spawnSync } from "node:child_process";

const root = resolve(import.meta.dirname, "..");
const sourceRoot = resolve(root, "src");
const manifest = resolve(root, "src-tauri", "Cargo.toml");

function fail(message) {
  console.error(`Architecture check failed: ${message}`);
  process.exit(1);
}

const metadataResult = spawnSync(
  "cargo",
  ["metadata", "--locked", "--format-version=1", "--manifest-path", manifest],
  {
    cwd: dirname(manifest),
    encoding: "utf8",
    maxBuffer: 16 * 1024 * 1024,
    shell: false,
  },
);

if (metadataResult.status !== 0) {
  fail(
    metadataResult.error?.message ||
      metadataResult.stderr.trim() ||
      "cargo metadata did not complete",
  );
}

const metadata = JSON.parse(metadataResult.stdout);
const workspaceIds = new Set(metadata.workspace_members);
const packages = new Map(
  metadata.packages
    .filter((pkg) => workspaceIds.has(pkg.id))
    .map((pkg) => [pkg.name, pkg]),
);
const expectedPackages = ["cleanup-core", "supa-diska-klinah", "windows-platform"];

if (
  packages.size !== expectedPackages.length ||
  expectedPackages.some((name) => !packages.has(name))
) {
  fail(`workspace packages must be exactly: ${expectedPackages.join(", ")}`);
}

function workspaceDependencies(packageName) {
  return packages
    .get(packageName)
    .dependencies.filter((dependency) => dependency.path)
    .map((dependency) => dependency.name)
    .sort();
}

const expectedEdges = new Map([
  ["supa-diska-klinah", ["windows-platform"]],
  ["windows-platform", ["cleanup-core"]],
  ["cleanup-core", []],
]);

for (const [packageName, expected] of expectedEdges) {
  const actual = workspaceDependencies(packageName);
  if (actual.join() !== expected.join()) {
    fail(`${packageName} workspace dependencies are ${actual.join(", ") || "none"}`);
  }
}

const coreDependencies = packages.get("cleanup-core").dependencies.map(({ name }) => name);
const forbiddenCoreDependency = coreDependencies.find(
  (name) => name === "tauri" || name.startsWith("tauri-") || name === "windows" || name.startsWith("windows-"),
);

if (forbiddenCoreDependency) {
  fail(`cleanup-core cannot depend on ${forbiddenCoreDependency}`);
}

function sourceFiles(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = resolve(directory, entry.name);
    if (entry.isDirectory()) return sourceFiles(path);
    return /\.(?:ts|tsx)$/.test(entry.name) ? [path] : [];
  });
}

function resolveImport(fromFile, specifier) {
  const base = resolve(dirname(fromFile), specifier);
  const candidates = extname(base)
    ? [base]
    : [base, `${base}.ts`, `${base}.tsx`, resolve(base, "index.ts"), resolve(base, "index.tsx")];
  return candidates.find((candidate) => existsSync(candidate) && statSync(candidate).isFile());
}

function area(path) {
  const parts = relative(sourceRoot, path).split(sep);
  if (parts[0] === "features") return { kind: "feature", name: parts[1] };
  return { kind: parts[0] };
}

const importPattern = /(?:import|export)\s+(?:[^'\"]*?\s+from\s+)?["']([^"']+)["']/g;

for (const file of sourceFiles(sourceRoot)) {
  const from = area(file);
  for (const match of readFileSync(file, "utf8").matchAll(importPattern)) {
    const specifier = match[1];
    if (!specifier.startsWith(".")) continue;
    const target = resolveImport(file, specifier);
    if (!target) fail(`${relative(root, file)} imports missing local module ${specifier}`);
    const to = area(target);

    if (from.kind === "shared" && (to.kind === "feature" || to.kind === "app")) {
      fail(`${relative(root, file)} crosses from shared into ${to.kind}`);
    }
    if (
      from.kind === "feature" &&
      (to.kind === "app" || (to.kind === "feature" && to.name !== from.name))
    ) {
      fail(`${relative(root, file)} crosses its feature boundary`);
    }
  }
}

console.log("Architecture boundaries verified.");
