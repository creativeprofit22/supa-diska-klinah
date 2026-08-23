import { existsSync, readFileSync } from "node:fs";

const requiredDocuments = [
  "docs/architecture.md",
  "docs/development.md",
  "docs/parity.md",
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

console.log("README documentation links verified.");
