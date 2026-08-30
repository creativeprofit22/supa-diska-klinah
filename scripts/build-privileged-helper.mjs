import { copyFileSync, mkdirSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const target = process.env.TAURI_ENV_TARGET_TRIPLE;
const debug = process.env.TAURI_ENV_DEBUG;
const supportedTargets = new Set([
  "x86_64-pc-windows-msvc",
  "aarch64-pc-windows-msvc",
]);

if (!supportedTargets.has(target)) {
  throw new Error("TAURI_ENV_TARGET_TRIPLE must name a supported Windows MSVC target");
}
if (debug !== undefined && debug !== "true") {
  throw new Error("TAURI_ENV_DEBUG must be true when set");
}

const profile = debug === "true" ? "debug" : "release";
const cargoArgs = [
  "build",
  "--manifest-path",
  resolve(root, "src-tauri/Cargo.toml"),
  "--package",
  "privileged-helper",
  "--target",
  target,
  "--locked",
];
if (profile === "release") cargoArgs.push("--release");

const build = spawnSync("cargo", cargoArgs, {
  cwd: root,
  shell: false,
  stdio: "inherit",
});
if (build.error) throw build.error;
if (build.status !== 0) process.exit(build.status ?? 1);

const source = resolve(
  root,
  "src-tauri/target",
  target,
  profile,
  "privileged-helper.exe",
);
const destination = resolve(
  root,
  "src-tauri/binaries",
  `supa-diska-klinah-privileged-helper-${target}.exe`,
);
mkdirSync(dirname(destination), { recursive: true });
copyFileSync(source, destination);
console.log(`Prepared privileged helper: ${destination}`);
