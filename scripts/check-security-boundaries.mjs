import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const read = (path) => readFileSync(resolve(root, path), "utf8");
const fail = (message) => {
  console.error(`Security boundary check failed: ${message}`);
  process.exit(1);
};

const build = read("src-tauri/build.rs");
const app = read("src-tauri/src/lib.rs");
const capability = JSON.parse(read("src-tauri/capabilities/main.json"));
const tauriConfig = JSON.parse(read("src-tauri/tauri.conf.json"));
const appManifest = read("src-tauri/windows-app-manifest.xml");
const helperManifest = read(
  "src-tauri/crates/privileged-helper/windows-app-manifest.xml",
);
const commandSource = read("src-tauri/src/commands/security.rs");
const brokerSource = read("src-tauri/crates/windows-platform/src/security/broker.rs");
const helperSource = read("src-tauri/crates/windows-platform/src/security/helper.rs");
const protocolSource = read("src-tauri/crates/windows-platform/src/security/protocol.rs");
const ciWorkflow = read(".github/workflows/ci.yml");
const nativeSmokeCi = read("scripts/smoke-native-ci.ps1");
const focus = process.argv[2] ?? "all";
const focusMessages = {
  all: "Windows and Tauri security boundaries verified.",
  privilege: "Standard-integrity main-process boundary verified.",
  commands: "Typed command registration and validation boundary verified.",
  webview: "Local-only webview and default-deny capability boundary verified.",
  helper: "Allowlisted authenticated helper transport boundary verified.",
};
if (!(focus in focusMessages)) fail(`unknown check focus: ${focus}`);

const cargoFiles = [
  "src-tauri/Cargo.toml",
  "src-tauri/crates/cleanup-core/Cargo.toml",
  "src-tauri/crates/privileged-helper/Cargo.toml",
  "src-tauri/crates/windows-platform/Cargo.toml",
].map(read);

const manifestMatch = build.match(/\.commands\(&\[([\s\S]*?)\]\)/);
const handlerMatch = app.match(/generate_handler!\[([\s\S]*?)\]/);
if (!manifestMatch || !handlerMatch) fail("command registrations could not be parsed");
const quoted = (text) => [...text.matchAll(/"([a-z0-9_]+)"/g)].map((match) => match[1]);
const manifestCommands = quoted(manifestMatch[1]).sort();
const handlerCommands = handlerMatch[1]
  .split(",")
  .map((command) => command.trim().split("::").at(-1))
  .filter(Boolean)
  .sort();
const permissionCommands = capability.permissions
  .filter((permission) => permission.startsWith("allow-"))
  .map((permission) => permission.slice(6).replaceAll("-", "_"))
  .sort();

for (const [name, commands] of [
  ["invoke handler", handlerCommands],
  ["capability permissions", permissionCommands],
]) {
  if (commands.join() !== manifestCommands.join()) {
    fail(`${name} drifted from the application command manifest`);
  }
}

if (
  capability.local !== true ||
  JSON.stringify(capability.webviews) !== JSON.stringify(["main"]) ||
  "windows" in capability ||
  "remote" in capability ||
  JSON.stringify(capability.platforms) !== JSON.stringify(["windows"])
) {
  fail("the capability must target only the local Windows main webview");
}

const forbiddenPermission = capability.permissions.find((permission) =>
  /(?:^core:default$|shell|filesystem|fs:|process|sidecar)/i.test(permission),
);
if (forbiddenPermission) fail(`forbidden capability permission: ${forbiddenPermission}`);

if (
  tauriConfig.app.security.assetProtocol?.enable !== false ||
  tauriConfig.app.withGlobalTauri === true
) {
  fail("asset protocol and global Tauri injection must remain disabled");
}
if (tauriConfig.bundle.externalBin?.length !== 1) {
  fail("exactly one reviewed privileged helper sidecar must be bundled");
}

if (!/requestedExecutionLevel\s+level="asInvoker"\s+uiAccess="false"/.test(appManifest)) {
  fail("main manifest must explicitly request asInvoker");
}
if (/requireAdministrator|highestAvailable/.test(appManifest)) {
  fail("main manifest may not request elevation");
}
if (
  !/requestedExecutionLevel\s+level="requireAdministrator"\s+uiAccess="false"/.test(
    helperManifest,
  )
) {
  fail("helper manifest must request elevation without UI access");
}
if (
  app.indexOf("require_standard_user()") < 0 ||
  app.indexOf("require_standard_user()") > app.indexOf("tauri::Builder::default()")
) {
  fail("main startup must reject elevation before constructing Tauri");
}
if (
  !/run:\s+\.\/scripts\/smoke-native-ci\.ps1 -Target \$\{\{ matrix\.target \}\}/.test(
    ciWorkflow,
  ) ||
  !/New-LocalUser -Name \$username -Password \$securePassword/.test(nativeSmokeCi) ||
  !/-Credential \$credential -LoadUserProfile/.test(nativeSmokeCi) ||
  !/Remove-LocalUser -Name \$username/.test(nativeSmokeCi)
) {
  fail("native CI must launch through a temporary standard-user account");
}
if (cargoFiles.some((cargo) => /tauri-plugin-(?:shell|fs)/.test(cargo))) {
  fail("generic shell and filesystem plugins are forbidden");
}

if (
  !/derive\(Debug, Deserialize\)/.test(commandSource) ||
  !/deny_unknown_fields/.test(commandSource) ||
  commandSource.indexOf("RestorePointDescription::parse") >
    commandSource.indexOf("broker::create_system_restore_point")
) {
  fail("frontend command input must be typed and validated before broker access");
}
if (
  !/TOKEN_BYTES: usize = 32/.test(protocolSource) ||
  !/MAX_FRAME_BYTES: usize = 4 \* 1024/.test(protocolSource) ||
  !/AUTHORIZATION_LIFETIME_SECONDS: u64 = 60/.test(protocolSource) ||
  !/SOCKET_TIMEOUT: Duration = Duration::from_secs\(120\)/.test(brokerSource) ||
  !/HANDSHAKE_DEADLINE: Duration = Duration::from_secs\(90\)/.test(brokerSource) ||
  !/PrivilegedOperation::CreateSystemRestorePoint/.test(helperSource)
 ) {
  fail("helper authentication, bounds, timeouts, or operation allowlist drifted");
}

console.log(focusMessages[focus]);
