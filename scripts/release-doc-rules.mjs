// Documentation-as-release-gate rules (pure; wired into check-docs.mjs).
// Each function takes file contents and returns failure messages.

import { declaredSigningMode } from "./release-assets.mjs";

/** Required docs → required headings. Each doc must also be linked from the README. */
export const RELEASE_DOCS = {
  "docs/installation.md": ["## Requirements", "## Download", "## This release is not code-signed", "## Verify your download", "## Install", "## Uninstall"],
  "docs/user-guide.md": ["## Getting around", "## Storage", "## System", "## Protection", "## Settings", "## Language", "## Getting help"],
  "docs/privacy.md": ["## What stays on this device", "## Network connections", "## Where data is stored", "## Removing your data"],
  "docs/updates.md": ["## How updates work", "## Turning update checks on", "## Installing an update", "## If an update does not finish", "## What is verified", "## Key rotation", "## Moving from unsigned to signed releases"],
  "docs/localization.md": ["## Supported languages", "## How the language is chosen", "## Adding or changing text", "## Adding a language", "## Review rule"],
  "docs/accessibility.md": ["## What is supported", "## Keyboard", "## Screen readers", "## Text size and zoom", "## Motion and contrast", "## Automated checks", "## Known gaps"],
  "docs/release.md": ["## Signing modes", "## Secrets and variables", "## Key custody", "## Cutting a release", "## What the pipeline verifies", "## Verifying a published release", "## Rollback", "## Turning on code signing"],
  "docs/cleanup-recovery.md": [],
  "docs/verification/release.md": ["## Status", "## Automated gates", "## Release candidate acceptance", "## Manual items", "## Signed release"],
  "docs/verification/accessibility.md": ["## Automated", "## Manual Narrator pass", "## 200 % text scale"],
};

export function releaseDocFailures(readme, docs) {
  const failures = [];
  for (const [path, headings] of Object.entries(RELEASE_DOCS)) {
    const text = docs[path];
    if (text === undefined) {
      failures.push(`missing ${path}`);
      continue;
    }
    if (!readme.includes(`](${path})`)) failures.push(`README does not link ${path}`);
    for (const heading of headings) {
      if (!text.split(/\r?\n/).includes(heading)) failures.push(`${path} lacks ${heading}`);
    }
  }
  return failures;
}

/** The README status list must not describe shipped work as deferred. */
export function readmeStatusFailures(readme) {
  const match = /^## Current status\s*$([\s\S]*?)(?=^## |(?![\s\S]))/m.exec(readme);
  if (!match) return ["README lacks ## Current status"];
  return /\bdeferred\b/i.test(match[1]) ? ["README status list still says something is deferred"] : [];
}

/** package.json, tauri.conf.json and the app Cargo.toml must agree on the version. */
export function versionFailures({ packageJson, tauriConf, cargoToml }) {
  const versions = {
    "package.json": JSON.parse(packageJson).version,
    "src-tauri/tauri.conf.json": JSON.parse(tauriConf).version,
    "src-tauri/Cargo.toml": /^\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m.exec(cargoToml)?.[1],
  };
  const distinct = new Set(Object.values(versions));
  return distinct.size === 1 && !distinct.has(undefined)
    ? []
    : [`versions differ: ${Object.entries(versions).map(([file, version]) => `${file}=${version}`).join(", ")}`];
}

/** Every `Endpoint` variant in net.rs must appear as a backticked name in privacy.md. */
export function endpointVariants(netRs) {
  const body = /pub enum Endpoint \{([\s\S]*?)\n\}/.exec(netRs)?.[1];
  if (!body) return null;
  const withoutComments = body.replace(/\/\/.*$/gm, "");
  return [...withoutComments.matchAll(/^\s*([A-Z][A-Za-z0-9]*)\s*(?:[,({]|$)/gm)].map((match) => match[1]);
}

export function privacyFailures(privacy, netRs) {
  const variants = endpointVariants(netRs);
  if (!variants || variants.length === 0) return ["could not read the Endpoint enum from net.rs"];
  return variants.filter((name) => !privacy.includes(`\`${name}\``)).map((name) => `docs/privacy.md does not list endpoint \`${name}\``);
}

export const UNSIGNED_NOTICES = {
  "README.md": ["Releases are currently unsigned"],
  "docs/installation.md": ["## This release is not code-signed", "Windows protected your PC", "Unknown publisher", "SmartScreen", "Get-FileHash", "gh attestation verify"],
  "docs/security.md": ["**Unsigned releases.**"],
  "src/features/settings/strings.ts": ["Releases are currently not code-signed", "Por ahora, las versiones no tienen firma de código"],
};

/**
 * The committed marker in docs/release.md is the source of truth (the CI
 * variable cannot be read offline; the release job checks the two agree).
 * Unsigned: every notice present. Authenticode: none of them left behind.
 */
export function signingModeFailures(releaseDoc, docs) {
  const mode = declaredSigningMode(releaseDoc);
  if (!mode) return ["docs/release.md lacks the line `Current signing mode: unsigned|authenticode`"];
  const failures = [];
  for (const [path, notices] of Object.entries(UNSIGNED_NOTICES)) {
    const text = docs[path];
    if (text === undefined) {
      failures.push(`missing ${path}`);
      continue;
    }
    for (const notice of notices) {
      const present = text.includes(notice);
      if (mode === "unsigned" && !present) failures.push(`${path} must say "${notice}" while releases are unsigned`);
      if (mode === "authenticode" && present) failures.push(`${path} still says "${notice}" but releases are signed`);
    }
  }
  return failures;
}
