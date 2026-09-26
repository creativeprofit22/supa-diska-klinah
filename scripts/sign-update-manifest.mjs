// Builds and signs the app-update manifest (update.json + update.json.sig).
//
// Uses node:crypto Ed25519 only (key helpers shared with rule-pack.mjs).
// The private key comes from the UPDATE_SIGNING_KEY environment variable
// (PKCS#8 PEM) and is never written to disk. After signing, the script
// verifies its own output against the public key embedded in the app
// (src-tauri/keys/update.pub) and fails on any mismatch, so a release can
// never publish a manifest the shipped app would reject.
//
//   UPDATE_SIGNING_KEY=<pem> node scripts/sign-update-manifest.mjs \
//     --installer <path to Supa-Diska-Klinah_<ver>_x64-setup.exe> \
//     --version <x.y.z> --signing-mode <unsigned|authenticode> \
//     [--thumbprint <40 hex, required for authenticode>] \
//     [--minimum-version <x.y.z>] [--valid-days <1-90>] --out-dir <dir>
//
// The manifest schema must match protection-core/src/update.rs exactly.

import { createHash } from "node:crypto";
import { readFileSync, statSync, writeFileSync } from "node:fs";
import { basename, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { signPack, verifyPack } from "./rule-pack.mjs";

const REPO_ROOT = resolve(import.meta.dirname, "..");
export const EMBEDDED_PUBLIC_KEY = resolve(REPO_ROOT, "src-tauri/keys/update.pub");
export const MAX_INSTALLER_BYTES = 512 * 1024 * 1024;
export const MAX_VALID_DAYS = 90;
const DAY = 24 * 60 * 60;
const VERSION = /^(0|[1-9]\d{0,8})\.(0|[1-9]\d{0,8})\.(0|[1-9]\d{0,8})$/;

export function parseVersion(text) {
  const match = VERSION.exec(text ?? "");
  if (!match) throw new Error(`not a strict x.y.z version: ${text}`);
  return match.slice(1).map(Number);
}

function compareVersions(a, b) {
  const [x, y] = [parseVersion(a), parseVersion(b)];
  for (let i = 0; i < 3; i += 1) if (x[i] !== y[i]) return x[i] - y[i];
  return 0;
}

export function installerName(version) {
  parseVersion(version);
  return `Supa-Diska-Klinah_${version}_x64-setup.exe`;
}

/**
 * Returns the exact manifest bytes. Field order is fixed so the bytes are
 * reproducible; the signature covers these bytes, not a re-serialisation.
 */
export function buildManifest({ version, minimumVersion, size, sha256, notBefore, validDays, signingMode, thumbprint }) {
  parseVersion(version);
  parseVersion(minimumVersion);
  if (compareVersions(minimumVersion, version) > 0) throw new Error("minimum version is newer than the release");
  if (!Number.isSafeInteger(size) || size <= 0 || size > MAX_INSTALLER_BYTES) throw new Error("installer size out of range");
  if (!/^[0-9a-f]{64}$/.test(sha256)) throw new Error("sha256 must be 64 lowercase hex characters");
  if (!Number.isInteger(validDays) || validDays < 1 || validDays > MAX_VALID_DAYS) {
    throw new Error(`valid days must be 1-${MAX_VALID_DAYS}`);
  }
  const manifest = {
    format: 1,
    version,
    minimumVersion,
    installer: { name: installerName(version), size, sha256 },
    notBefore,
    expires: notBefore + validDays * DAY,
  };
  if (signingMode === "unsigned") {
    if (thumbprint) throw new Error("unsigned releases must not carry a signer thumbprint");
    manifest.signing = "none";
  } else if (signingMode === "authenticode") {
    if (!/^[0-9A-F]{40}$/.test(thumbprint ?? "")) throw new Error("authenticode releases need a 40-character uppercase thumbprint");
    manifest.signing = "authenticode";
    manifest.signerThumbprint = thumbprint;
  } else {
    throw new Error(`signing mode must be "unsigned" or "authenticode", got ${JSON.stringify(signingMode)}`);
  }
  return Buffer.from(JSON.stringify(manifest), "utf8");
}

export function sha256File(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function parseArgs(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 2) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (!flag?.startsWith("--") || value === undefined) throw new Error(`bad argument near ${flag}`);
    options[flag.slice(2)] = value;
  }
  return options;
}

export function run(argv, env = process.env, now = Math.floor(Date.now() / 1000)) {
  const options = parseArgs(argv);
  for (const required of ["installer", "version", "signing-mode", "out-dir"]) {
    if (!options[required]) throw new Error(`--${required} is required`);
  }
  const privatePem = env.UPDATE_SIGNING_KEY;
  if (!privatePem) throw new Error("UPDATE_SIGNING_KEY is not set; refusing to publish an unsigned manifest");
  const publicHex = readFileSync(options["public-key"] ?? EMBEDDED_PUBLIC_KEY, "utf8");
  if (publicHex.trim() === "unconfigured") {
    throw new Error("src-tauri/keys/update.pub is unconfigured; generate the update key first (scripts/generate-update-key.mjs)");
  }
  const installer = resolve(options.installer);
  if (basename(installer) !== installerName(options.version)) {
    throw new Error(`installer must be named ${installerName(options.version)}`);
  }
  const bytes = buildManifest({
    version: options.version,
    minimumVersion: options["minimum-version"] ?? "0.1.0",
    size: statSync(installer).size,
    sha256: sha256File(installer),
    notBefore: now,
    validDays: Number(options["valid-days"] ?? MAX_VALID_DAYS),
    signingMode: options["signing-mode"],
    thumbprint: options.thumbprint,
  });
  const signature = signPack(privatePem, bytes);
  if (!verifyPack(publicHex, bytes, signature)) {
    throw new Error("the signing key does not match the public key embedded in the app; refusing to publish");
  }
  const outDir = resolve(options["out-dir"]);
  writeFileSync(resolve(outDir, "update.json"), bytes, { flag: "wx" });
  writeFileSync(resolve(outDir, "update.json.sig"), `${signature}\n`, { flag: "wx" });
  return { manifest: JSON.parse(bytes.toString("utf8")), signature };
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  try {
    const { manifest } = run(process.argv.slice(2));
    console.log(`Signed update manifest for ${manifest.version} (${manifest.signing}), expires ${new Date(manifest.expires * 1000).toISOString()}.`);
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
}
