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
const expectedPackages = [
  "cleanup-core",
  "privileged-helper",
  "protection-core",
  "supa-diska-klinah",
  "windows-platform",
];

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
  ["privileged-helper", ["windows-platform"]],
  ["windows-platform", ["cleanup-core", "protection-core"]],
  ["cleanup-core", []],
  // ADR 0003: portable rules, matching and evidence; no platform or app edges.
  ["protection-core", []],
]);

for (const [packageName, expected] of expectedEdges) {
  const actual = workspaceDependencies(packageName);
  if (actual.join() !== expected.join()) {
    fail(`${packageName} workspace dependencies are ${actual.join(", ") || "none"}`);
  }
}

for (const corePackage of ["cleanup-core", "protection-core"]) {
  const coreDependencies = packages.get(corePackage).dependencies.map(({ name }) => name);
  const forbiddenCoreDependency = coreDependencies.find(
    (name) => name === "tauri" || name.startsWith("tauri-") || name === "windows" || name.startsWith("windows-"),
  );
  if (forbiddenCoreDependency) {
    fail(`${corePackage} cannot depend on ${forbiddenCoreDependency}`);
  }
}

// ADR 0003: no HTTP client crates anywhere; the only network sink is WinHTTP in protection/net.rs.
const httpCrates = /^(?:reqwest|ureq|hyper|hyper-util|isahc|surf|attohttpc|minreq|curl|curl-sys|tauri-plugin-http|tauri-plugin-upload|tungstenite|tokio-tungstenite|yara|yara-x|yara-sys)$/;
for (const [packageName, pkg] of packages) {
  const httpDependency = pkg.dependencies.find(({ name }) => httpCrates.test(name));
  if (httpDependency) fail(`${packageName} cannot depend on ${httpDependency.name}; see ADR 0003`);
}

for (const packageName of ["cleanup-core", "privileged-helper", "protection-core", "windows-platform"]) {
  const dependency = packages
    .get(packageName)
    .dependencies.find(({ name }) => name === "tauri" || name.startsWith("tauri-"));
  if (dependency) fail(`${packageName} cannot depend on ${dependency.name}`);
}

const rustRoots = [
  resolve(root, "src-tauri/src"),
  resolve(root, "src-tauri/crates"),
];
const approvedProcessOwner = resolve(
  root,
  "src-tauri/crates/windows-platform/src/cleanup/build_artifacts.rs",
);
const approvedVendorOwner = resolve(root, "src-tauri/crates/windows-platform/src/storage/vendor_uninstall.rs");
const approvedBrokerOwner = resolve(root, "src-tauri/crates/windows-platform/src/security/broker.rs");
// ADR 0002: the only system-management process launch is the helper-side
// `<System32>\powercfg.exe /hibernate on|off` with fixed argv.
const approvedPowercfgOwner = resolve(root, "src-tauri/crates/windows-platform/src/power/elevated.rs");
// Only the #[cfg(test)] cancellation tests copy/compile this standalone fixture.
// Keep this exception exact: other tests, fixtures, and production files stay checked.
// ADR 0003: the single network sink, and the read-only process inventory.
const approvedNetworkOwner = resolve(root, "src-tauri/crates/windows-platform/src/protection/net.rs");
const protectionDir = resolve(root, "src-tauri/crates/windows-platform/src/protection");
const approvedProcessFixture = resolve(
  root,
  "src-tauri/crates/windows-platform/tests/fixtures/native-process-tree.rs",
);
const systemModuleDirs = [
  "startup_items", "services", "drivers", "firewall", "hosts", "privacy", "power", "restore",
  "updates", "scheduler", "system_change",
].map((name) => resolve(root, "src-tauri/crates/windows-platform/src", name));
const systemModuleFiles = new Set(
  ["optimizer.rs", "win_registry.rs", "os_info.rs", "security/system_changes.rs"].map((name) =>
    resolve(root, "src-tauri/crates/windows-platform/src", name),
  ),
);
function rustFiles(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = resolve(directory, entry.name);
    if (entry.isDirectory()) return rustFiles(path);
    return entry.name.endsWith(".rs") ? [path] : [];
  });
}
for (const directory of rustRoots) {
  for (const file of rustFiles(directory)) {
    const source = readFileSync(file, "utf8");
    if (
      /\bprocess\s*::\s*(?:Command|\*|\{[^}]*\bCommand\b)|\bCommand\s*::\s*new|\buse\s+std\s*::\s*process\s*(?:;|as)/.test(source) &&
      resolve(file) !== approvedVendorOwner &&
      resolve(file) !== approvedProcessOwner &&
      resolve(file) !== approvedPowercfgOwner &&
      resolve(file) !== approvedProcessFixture
    ) {
      fail(`${relative(root, file)} contains forbidden runtime process execution`);
    }
    if (resolve(file) === approvedPowercfgOwner) {
      const launches = source.match(/\bCommand\s*::\s*new\b/g) ?? [];
      if (
        launches.length !== 1 ||
        !/\["\/hibernate",\s*if enabled \{ "on" \} else \{ "off" \}\]/.test(source) ||
        !/const POWERCFG_EXE: &str = "powercfg\.exe";/.test(source) ||
        /\.arg\s*\(|\bformat!\s*\(/.test(source)
      ) {
        fail(`${relative(root, file)} must launch only powercfg.exe /hibernate on|off with fixed argv`);
      }
    }
    if (systemModuleFiles.has(resolve(file)) || systemModuleDirs.some((dir) => resolve(file).startsWith(dir + sep))) {
      if (/"(?:[^"\\]|\\.)*\b(?:cmd(?:\.exe)?|powershell(?:\.exe)?|pwsh(?:\.exe)?|schtasks(?:\.exe)?|netsh(?:\.exe)?|sc\.exe|reg\.exe|wmic(?:\.exe)?|pnputil(?:\.exe)?)\b/i.test(source)) {
        fail(`${relative(root, file)} references a shell or management CLI; use typed Windows APIs`);
      }
    }
    if (/\bWinHttp[A-Za-z]*\b|\bwinhttp\b|\bWinInet\b|\bInternet(?:Open|Connect|ReadFile)[AW]?\b|\bURLDownloadToFile[AW]?\b/.test(source) &&
        resolve(file) !== approvedNetworkOwner) {
      fail(`${relative(root, file)} uses a network API; only protection/net.rs may (ADR 0003)`);
    }
    // Raw sockets: only the helper broker/helper loopback transport is approved.
    const loopbackOwners = [approvedBrokerOwner, resolve(root, "src-tauri/crates/windows-platform/src/security/helper.rs")];
    if (/\bTcpStream\b|\bTcpListener\b|\bUdpSocket\b|\bWSAStartup\b/.test(source) &&
        !loopbackOwners.includes(resolve(file))) {
      fail(`${relative(root, file)} opens a socket; only the loopback helper transport may`);
    }
    if (loopbackOwners.includes(resolve(file)) && (/\bUNSPECIFIED\b|"0\.0\.0\.0/.test(source) || !/Ipv4Addr::LOCALHOST/.test(source))) {
      fail(`${relative(root, file)} must use loopback only`);
    }
    if (resolve(file).startsWith(protectionDir + sep) &&
        /\b(?:TerminateProcess|NtTerminateProcess|SuspendThread|NtSuspendProcess|DebugActiveProcess|WriteProcessMemory|CreateRemoteThread|PROCESS_TERMINATE|PROCESS_VM_READ|PROCESS_VM_WRITE|PROCESS_ALL_ACCESS)\b/.test(source)) {
      fail(`${relative(root, file)} must stay read-only toward other processes (ADR 0003)`);
    }
    // Detect imported/aliased APIs too, not just call expressions. Fixture paths get no native exception.
    if (/\b(?:CreateProcess(?:AsUser|WithLogon|WithToken)?[AW]?|ShellExecute(?:Ex)?[AW]?|WinExec|NtCreateUserProcess|RtlCreateUserProcess)\b/.test(source) &&
        ![approvedVendorOwner, approvedBrokerOwner, approvedProcessOwner].includes(resolve(file))) {
      fail(`${relative(root, file)} contains forbidden native process execution`);
    }
  }
}

function sourceFiles(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = resolve(directory, entry.name);
    if (entry.isDirectory()) return sourceFiles(path);
    return /\.(?:ts|tsx)$/.test(entry.name) ? [path] : [];
  });
}

function resolveImport(fromFile, specifier) {
  // Vite's raw-text suffix does not change the file's existence or boundary ownership.
  const base = resolve(dirname(fromFile), specifier.replace(/\?raw$/, ""));
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
const approvedFeatureBridges = new Set([
  "src/features/build-artifacts/ArtifactBudgetSettings.tsx->src/features/cleanup/api/previewCleanup.ts",
  "src/features/build-artifacts/BuildArtifactCoordinator.tsx->src/features/cleanup/api/previewCleanup.ts",
  "src/features/build-artifacts/BuildArtifactCoordinator.tsx->src/features/cleanup/format.ts",
  "src/features/drives/DriveInventoryPage.tsx->src/features/cleanup/format.ts",
  // Reuse the cleanup API's project-root registry, like the coordinator/settings consumers above.
  // Only this hook-to-API edge is approved, not the surrounding features.
  "src/features/build-artifacts/useProjectRoots.ts->src/features/cleanup/api/previewCleanup.ts",
  "src/features/cleanup/CleanupPreviewPage.tsx->src/features/build-artifacts/BuildArtifactCoordinator.tsx",
  "src/features/settings/SettingsPage.tsx->src/features/build-artifacts/ArtifactBudgetSettings.tsx",
]);

for (const file of sourceFiles(sourceRoot)) {
  const from = area(file);
  for (const match of readFileSync(file, "utf8").matchAll(importPattern)) {
    const specifier = match[1];
    if (!specifier.startsWith(".")) continue;
    const target = resolveImport(file, specifier);
    if (!target) fail(`${relative(root, file)} imports missing local module ${specifier}`);
    const to = area(target);
    const bridge = `${relative(root, file).split(sep).join("/")}->${relative(root, target).split(sep).join("/")}`;

    if (from.kind === "shared" && (to.kind === "feature" || to.kind === "app")) {
      fail(`${relative(root, file)} crosses from shared into ${to.kind}`);
    }
    if (
      from.kind === "feature" &&
      (to.kind === "app" || (to.kind === "feature" && to.name !== from.name)) &&
      !approvedFeatureBridges.has(bridge)
    ) {
      fail(`${relative(root, file)} crosses its feature boundary`);
    }
  }
}

// ADR 0003: the webview never talks to the network; connect-src stays IPC-only.
const tauriConfig = JSON.parse(readFileSync(resolve(root, "src-tauri/tauri.conf.json"), "utf8"));
const connectSources = (csp) => csp?.match(/connect-src ([^;]*)/)?.[1].trim().split(/\s+/) ?? [];
if (connectSources(tauriConfig.app.security.csp).join(" ") !== "ipc: http://ipc.localhost") {
  fail("production CSP connect-src must be exactly 'ipc: http://ipc.localhost'");
}
if (connectSources(tauriConfig.app.security.devCsp).some((source) => !/^(?:'self'|ipc:|http:\/\/ipc\.localhost|ws:\/\/127\.0\.0\.1:\d+)$/.test(source))) {
  fail("development CSP connect-src may only add the local dev server");
}
// ADR 0003: the embedded WebView2 engine must not make its own background
// connections. Setting these args replaces wry's defaults, so they are restated.
for (const window of tauriConfig.app.windows) {
  const args = (window.additionalBrowserArgs ?? "").split(/\s+/);
  for (const required of ["--disable-background-networking", "--disable-component-update", "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection"]) {
    if (!args.includes(required)) fail(`window "${window.label}" must set ${required} in additionalBrowserArgs`);
  }
}
for (const file of sourceFiles(sourceRoot)) {
  if (/\b(?:fetch\s*\(|XMLHttpRequest|WebSocket\s*\(|EventSource\s*\(|navigator\.sendBeacon)/.test(readFileSync(file, "utf8"))) {
    fail(`${relative(root, file)} makes a network request from the webview; use a typed command`);
  }
}

// ADR 0003: protection UI never claims certification or safety. Only rendered
// components are checked; types.ts mirrors backend codes, and tests assert absence.
const protectionUi = resolve(sourceRoot, "features/protection");
for (const file of (existsSync(protectionUi) ? sourceFiles(protectionUi) : []).filter((path) => /\.tsx$/.test(path) && !/\.test\.tsx$/.test(path))) {
  const strings = [...readFileSync(file, "utf8").matchAll(/"([^"\n]*)"|`([^`]*)`|>([^<>{}\n]+)</g)]
    .map((match) => match[1] ?? match[2] ?? match[3]);
  const claim = strings.find((text) => /\b(?:clean|safe|certified|protected|virus-free|malware-free|secure device)\b/i.test(text));
  if (claim) fail(`${relative(root, file)} uses certification wording: "${claim.trim().slice(0, 80)}"`);
}

console.log("Architecture boundaries verified.");
