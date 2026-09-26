import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";
import {
  endpointVariants,
  privacyFailures,
  readmeStatusFailures,
  RELEASE_DOCS,
  releaseDocFailures,
  signingModeFailures,
  UNSIGNED_NOTICES,
  versionFailures,
} from "./release-doc-rules.mjs";

const root = resolve(import.meta.dirname, "..");

test("README status must not call shipped work deferred", () => {
  assert.deepEqual(readmeStatusFailures("# X\n\n## Current status\n\n- Installer ships.\n\n## Quick start\n\nDeferred elsewhere is fine.\n"), []);
  assert.equal(readmeStatusFailures("## Current status\n\n- Updater intentionally deferred.\n\n## Next\n").length, 1);
  assert.equal(readmeStatusFailures("## Current status\n\n- Deferred at the end").length, 1);
  assert.equal(readmeStatusFailures("# no status").length, 1);
});

test("versions must agree", () => {
  const files = (a, b, c) => ({ packageJson: `{"version":"${a}"}`, tauriConf: `{"version":"${b}"}`, cargoToml: `[package]\nname = "x"\nversion = "${c}"\n` });
  assert.deepEqual(versionFailures(files("0.2.0", "0.2.0", "0.2.0")), []);
  assert.match(versionFailures(files("0.2.0", "0.2.1", "0.2.0"))[0], /tauri\.conf\.json=0\.2\.1/);
  assert.equal(versionFailures({ ...files("1.0.0", "1.0.0", "1.0.0"), cargoToml: "[workspace]\n" }).length, 1);
});

test("reads Endpoint variants from the real net.rs", () => {
  const net = readFileSync(resolve(root, "src-tauri/crates/windows-platform/src/protection/net.rs"), "utf8");
  assert.deepEqual(endpointVariants(net), ["RulePack", "RuleSignature", "PasswordRange", "UpdateManifest", "UpdateSignature", "UpdateInstaller"]);
  const fake = "pub enum Endpoint {\n    /// Doc\n    Alpha,\n    Beta([u8; 5]),\n    // Gamma is gone\n    Delta(AppVersion),\n}\n";
  assert.deepEqual(endpointVariants(fake), ["Alpha", "Beta", "Delta"]);
  assert.deepEqual(privacyFailures("`Alpha` `Beta`", fake), ["docs/privacy.md does not list endpoint `Delta`"]);
  assert.equal(privacyFailures("", "no enum").length, 1);
});

test("signing-mode docs follow the committed marker", () => {
  const allNotices = Object.fromEntries(Object.entries(UNSIGNED_NOTICES).map(([path, notices]) => [path, notices.join("\n")]));
  assert.deepEqual(signingModeFailures("Current signing mode: unsigned", allNotices), []);
  const missing = { ...allNotices, "README.md": "Releases are signed" };
  assert.match(signingModeFailures("Current signing mode: unsigned", missing).join("\n"), /README\.md must say/);
  assert.match(signingModeFailures("Current signing mode: authenticode", allNotices).join("\n"), /still says .* but releases are signed/);
  const clean = Object.fromEntries(Object.keys(UNSIGNED_NOTICES).map((path) => [path, "signed"]));
  assert.deepEqual(signingModeFailures("Current signing mode: authenticode", clean), []);
  assert.match(signingModeFailures("no marker", clean)[0], /lacks the line/);
  const { ["docs/security.md"]: _gone, ...withoutSecurity } = clean;
  assert.deepEqual(signingModeFailures("Current signing mode: authenticode", withoutSecurity), ["missing docs/security.md"]);
});

test("the in-app settings catalog follows the signing mode", () => {
  const catalog = "src/features/settings/strings.ts";
  const en = "Releases are currently not code-signed";
  const es = "Por ahora, las versiones no tienen firma de código";
  const allNotices = Object.fromEntries(Object.entries(UNSIGNED_NOTICES).map(([path, notices]) => [path, notices.join("\n")]));
  const clean = Object.fromEntries(Object.keys(UNSIGNED_NOTICES).map((path) => [path, "signed"]));
  const real = readFileSync(resolve(root, catalog), "utf8");
  assert.ok(real.includes(en) && real.includes(es));

  assert.deepEqual(signingModeFailures("Current signing mode: unsigned", { ...allNotices, [catalog]: real }), []);
  assert.deepEqual(signingModeFailures("Current signing mode: unsigned", { ...allNotices, [catalog]: en }), [
    `${catalog} must say "${es}" while releases are unsigned`,
  ]);
  assert.deepEqual(signingModeFailures("Current signing mode: authenticode", { ...clean, [catalog]: real }), [
    `${catalog} still says "${en}" but releases are signed`,
    `${catalog} still says "${es}" but releases are signed`,
  ]);
  assert.deepEqual(signingModeFailures("Current signing mode: authenticode", { ...clean, [catalog]: "Releases are code-signed." }), []);
});

test("required docs need their headings and a README link", () => {
  const docs = Object.fromEntries(Object.entries(RELEASE_DOCS).map(([path, headings]) => [path, headings.join("\n")]));
  const readme = Object.keys(RELEASE_DOCS).map((path) => `[x](${path})`).join("\n");
  assert.deepEqual(releaseDocFailures(readme, docs), []);
  const withoutRollback = { ...docs, "docs/release.md": docs["docs/release.md"].replace("## Rollback", "## Undo") };
  assert.deepEqual(releaseDocFailures(readme, withoutRollback), ["docs/release.md lacks ## Rollback"]);
  assert.deepEqual(releaseDocFailures(readme.replace("](docs/privacy.md)", ""), docs), ["README does not link docs/privacy.md"]);
  const { ["docs/updates.md"]: _removed, ...rest } = docs;
  assert.deepEqual(releaseDocFailures(readme, rest), ["missing docs/updates.md"]);
  // Headings must be whole lines, not substrings.
  const inline = { ...docs, "docs/privacy.md": docs["docs/privacy.md"].replace("## Network connections", "See ## Network connections") };
  assert.deepEqual(releaseDocFailures(readme, inline), ["docs/privacy.md lacks ## Network connections"]);
});
