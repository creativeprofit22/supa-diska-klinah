import { existsSync, readFileSync } from "node:fs";
import {
  privacyFailures,
  readmeStatusFailures,
  RELEASE_DOCS,
  releaseDocFailures,
  signingModeFailures,
  UNSIGNED_NOTICES,
  versionFailures,
} from "./release-doc-rules.mjs";

const requiredDocuments = [
  "docs/architecture.md",
  "docs/build-artifact-budgets.md",
  "docs/cleanup-rules.md",
  "docs/development.md",
  "docs/parity.md",
  "docs/performance.md",
  "docs/verification/performance.md",
  "docs/project-artifacts.md",
  "docs/release-checklist.md",
  "docs/security.md",
  "docs/storage.md",
  "docs/system-management.md",
  "docs/system-management-admin.md",
  "docs/verification/storage-parity.md",
  "docs/verification/system-management.md",
  "docs/protection.md",
  "docs/verification/protection.md",
  "docs/licensing.md",
  "docs/adr/0001-modular-boundaries.md",
  "docs/adr/0002-system-change-helper.md",
  "docs/adr/0003-local-first-protection.md",
  "CONTRIBUTING.md",
  "LICENSE",
  "THIRD_PARTY_NOTICES.md",
];
const readme = readFileSync("README.md", "utf8");
const missing = requiredDocuments.filter(
  (path) => !existsSync(path) || !readme.includes(`](${path})`),
);

if (missing.length) {
  console.error(`Documentation check failed: missing or unlinked ${missing.join(", ")}`);
  process.exit(1);
}

const security = readFileSync("docs/security.md", "utf8");
const requiredSecuritySections = [
  "## Scope and assets",
  "## Trust boundaries",
  "## Attacker model and assumptions",
  "## Privileged-operation inventory",
  "## Failure modes and recovery",
  "## Residual risks",
];
const missingSecuritySections = requiredSecuritySections.filter(
  (heading) => !security.includes(heading),
);
if (missingSecuritySections.length) {
  console.error(
    `Documentation check failed: security model lacks ${missingSecuritySections.join(", ")}`,
  );
  process.exit(1);
}

const artifacts = readFileSync("docs/build-artifact-budgets.md", "utf8");
for (const required of [
  "cargo.exe",
  "node.exe",
  "automatic `cargo clean`",
  "protected floor",
  "watcher silence",
  "seven days",
  "Cancellation",
  "undo",
]) {
  if (!artifacts.includes(required)) {
    console.error(`Documentation check failed: artifact budget guide lacks ${required}`);
    process.exit(1);
  }
}
const storage = readFileSync("docs/storage.md", "utf8");
const verification = readFileSync("docs/verification/storage-parity.md", "utf8");
for (const heading of ["## Choose the right tool", "## Scan, inspect, then decide", "## Recovery is not reclaimed space", "## Installed programs are different"]) {
  if (!storage.includes(heading)) {
    console.error(`Documentation check failed: storage guide lacks ${heading}`);
    process.exit(1);
  }
}
for (const page of ["disk-analyzer-readonly", "large-files", "cleaner", "duplicate-empty", "browser", "uninstaller", "storage-ui"]) {
  if (!existsSync(`docs/verification/${page}.md`) || !verification.includes(`](${page}.md)`)) {
    console.error(`Documentation check failed: missing or unlinked storage evidence: ${page}`);
    process.exit(1);
  }
}
const performance = readFileSync("docs/performance.md", "utf8");
for (const heading of [
  "## Methodology",
  "## Hardware",
  "## Corpora",
  "## How to run",
  "## Results",
  "## Budgets",
  "## Accepted and rejected optimizations",
  "## Limitations",
]) {
  if (!performance.includes(heading)) {
    console.error(`Documentation check failed: performance guide lacks ${heading}`);
    process.exit(1);
  }
}
// Documentation as a release gate: required release docs, current README status,
// one version everywhere, every network endpoint disclosed, and signing-mode notices
// matching the committed mode.
{
  const optional = (path) => (existsSync(path) ? readFileSync(path, "utf8") : undefined);
  const docs = Object.fromEntries(
    [...new Set([...Object.keys(RELEASE_DOCS), ...Object.keys(UNSIGNED_NOTICES)])].map((path) => [path, optional(path)]),
  );
  const releaseFailures = [
    ...releaseDocFailures(readme, docs),
    ...readmeStatusFailures(readme),
    ...versionFailures({
      packageJson: readFileSync("package.json", "utf8"),
      tauriConf: readFileSync("src-tauri/tauri.conf.json", "utf8"),
      cargoToml: readFileSync("src-tauri/Cargo.toml", "utf8"),
    }),
    ...privacyFailures(docs["docs/privacy.md"] ?? "", readFileSync("src-tauri/crates/windows-platform/src/protection/net.rs", "utf8")),
    ...signingModeFailures(docs["docs/release.md"] ?? "", docs),
  ];
  if (releaseFailures.length) {
    console.error(`Documentation check failed:\n  ${releaseFailures.join("\n  ")}`);
    process.exit(1);
  }
}
console.log("Threat model, privilege inventory, recovery, storage evidence, performance, release docs, signing-mode notices, and README links verified.");
