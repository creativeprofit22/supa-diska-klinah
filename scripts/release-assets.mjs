// Shared, pure release-asset rules used by the publish-side scripts and their
// tests. The build side (stage-release-assets.ps1) produces exactly this layout:
//
//   Supa-Diska-Klinah_<ver>_x64-setup.exe   installer (fixed name the updater expects)
//   update.json / update.json.sig            Ed25519-signed update manifest
//   SHA256SUMS                               "<sha256>  <file>" per line, sorted
//   dependency-inventory.json                locked Rust + npm dependency list

import { createHash } from "node:crypto";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { verifyPack } from "./rule-pack.mjs";
import { installerName, parseVersion } from "./sign-update-manifest.mjs";

export const SIGNING_MODES = ["unsigned", "authenticode"];
export const SIGNING_MODE_MARKER = /^Current signing mode: `?(unsigned|authenticode)`?\s*$/m;

/** Reads the committed signing-mode marker from docs/release.md text. */
export function declaredSigningMode(releaseDoc) {
  return SIGNING_MODE_MARKER.exec(releaseDoc)?.[1] ?? null;
}

export function assertSigningMode(mode, releaseDoc) {
  if (!SIGNING_MODES.includes(mode)) {
    throw new Error(`WINDOWS_SIGNING_MODE must be "unsigned" or "authenticode", got ${JSON.stringify(mode ?? "")}`);
  }
  const declared = declaredSigningMode(releaseDoc);
  if (declared !== mode) {
    throw new Error(`WINDOWS_SIGNING_MODE is "${mode}" but docs/release.md declares "${declared ?? "nothing"}"; update one to match`);
  }
  return mode;
}

/** `v1.2.3` → `1.2.3`; anything else is refused. */
export function versionFromTag(tag) {
  const version = /^v(.+)$/.exec(tag ?? "")?.[1];
  if (!version) throw new Error(`release tags must look like v1.2.3, got ${JSON.stringify(tag)}`);
  parseVersion(version);
  return version;
}

export function expectedFiles(version) {
  return [installerName(version), "SHA256SUMS", "dependency-inventory.json", "update.json", "update.json.sig"].sort();
}

export function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

export function parseSums(text) {
  const entries = new Map();
  for (const line of text.split(/\r?\n/)) {
    if (!line) continue;
    const match = /^([0-9a-f]{64}) {2}([A-Za-z0-9._-]+)$/.exec(line);
    if (!match || entries.has(match[2])) throw new Error(`malformed SHA256SUMS line: ${JSON.stringify(line)}`);
    entries.set(match[2], match[1]);
  }
  return entries;
}

/**
 * Re-verifies a staged release directory. Returns the parsed manifest.
 * Throws on any mismatch; publishing must not proceed.
 */
export function verifyReleaseDir({ dir, tag, mode, publicKeyHex, releaseDoc, now = Math.floor(Date.now() / 1000) }) {
  assertSigningMode(mode, releaseDoc);
  const version = versionFromTag(tag);
  const present = readdirSync(dir).sort();
  const expected = expectedFiles(version);
  if (JSON.stringify(present) !== JSON.stringify(expected)) {
    throw new Error(`release assets must be exactly ${expected.join(", ")}; found ${present.join(", ")}`);
  }
  const read = (name) => readFileSync(join(dir, name));
  const sums = parseSums(read("SHA256SUMS").toString("utf8"));
  const summed = expected.filter((name) => name !== "SHA256SUMS");
  if (JSON.stringify([...sums.keys()].sort()) !== JSON.stringify(summed)) {
    throw new Error("SHA256SUMS must list every other release asset exactly once");
  }
  for (const [name, digest] of sums) {
    if (sha256(read(name)) !== digest) throw new Error(`checksum mismatch for ${name}`);
  }

  if (publicKeyHex.trim() === "unconfigured") throw new Error("the embedded update key is unconfigured");
  const manifestBytes = read("update.json");
  if (!verifyPack(publicKeyHex, manifestBytes, read("update.json.sig").toString("utf8"))) {
    throw new Error("update.json signature does not verify against the embedded update key");
  }
  const manifest = JSON.parse(manifestBytes.toString("utf8"));
  const installer = read(installerName(version));
  if (manifest.version !== version) throw new Error(`manifest version ${manifest.version} does not match tag ${tag}`);
  if (manifest.installer?.name !== installerName(version)) throw new Error("manifest names a different installer");
  if (manifest.installer.size !== installer.length || manifest.installer.sha256 !== sha256(installer)) {
    throw new Error("manifest size or SHA-256 does not match the installer");
  }
  const expectedSigning = mode === "unsigned" ? "none" : "authenticode";
  if (manifest.signing !== expectedSigning) {
    throw new Error(`manifest signing is "${manifest.signing}" but the release mode is ${mode}`);
  }
  if (!(manifest.notBefore <= now + 600 && now < manifest.expires)) throw new Error("manifest is not currently valid");
  return manifest;
}

/** Title and notes for the GitHub release. Unsigned releases say so plainly. */
export function releaseText({ tag, mode, repository }) {
  const version = versionFromTag(tag);
  const title = mode === "unsigned" ? `Supa Diska Klinah ${version} (unsigned)` : `Supa Diska Klinah ${version}`;
  const verify = [
    "## Verify your download",
    "",
    "1. In PowerShell: `Get-FileHash .\\" + installerName(version) + " -Algorithm SHA256` and compare with `SHA256SUMS`.",
    `2. Optional, with the GitHub CLI: \`gh attestation verify ${installerName(version)} --repo ${repository}\` proves this file was built by this repository's CI.`,
  ];
  const unsigned = [
    "## This release is not code-signed",
    "",
    "Windows will show **\"Windows protected your PC\"** (SmartScreen) and **\"Publisher: Unknown\"** (UAC) when you run the installer.",
    "Verify the download as described below before choosing **More info → Run anyway**.",
    "See docs/installation.md for details.",
    "",
  ];
  const notes = [...(mode === "unsigned" ? unsigned : []), ...verify, ""].join("\n");
  return { title, notes };
}
