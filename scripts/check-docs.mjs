import { existsSync, readFileSync } from "node:fs";

const requiredDocuments = [
  "docs/architecture.md",
  "docs/development.md",
  "docs/parity.md",
  "docs/security.md",
  "docs/licensing.md",
  "docs/adr/0001-modular-boundaries.md",
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

console.log("Threat model, privilege inventory, recovery, and README links verified.");
