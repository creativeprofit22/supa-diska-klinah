import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";
import { hygieneFailures, jobPermissions, jobs, releaseFailures } from "./workflow-rules.mjs";

const root = resolve(import.meta.dirname, "..");
const ci = readFileSync(resolve(root, ".github/workflows/ci.yml"), "utf8");
const scripts = {
  verify: readFileSync(resolve(root, "scripts/verify-windows-release.ps1"), "utf8"),
  prepare: readFileSync(resolve(root, "scripts/prepare-windows-signing.ps1"), "utf8"),
};

function mutated(from, to) {
  assert.ok(ci.includes(from), `fixture text missing: ${from}`);
  return ci.replace(from, to);
}

test("the committed workflow satisfies every rule", () => {
  assert.deepEqual(hygieneFailures("ci.yml", ci), []);
  assert.deepEqual(releaseFailures(ci, scripts), []);
  assert.deepEqual(jobs(ci).map((job) => job.name), ["quality", "native-smoke", "dependency-audit", "windows-release-build", "windows-release-publish"]);
});

test("parses job permissions", () => {
  assert.deepEqual(jobPermissions("    permissions:\n      contents: read\n      id-token: write\n    steps:"), { contents: "read", "id-token": "write" });
  assert.deepEqual(jobPermissions("    permissions: write-all"), { _scalar: "write-all" });
  assert.equal(jobPermissions("    steps:"), null);
});

test("hygiene: per-job permissions and non-persisted checkout credentials", () => {
  const noPerms = mutated("    timeout-minutes: 45\n    permissions:\n      contents: read\n", "    timeout-minutes: 45\n");
  assert.match(hygieneFailures("ci.yml", noPerms).join("\n"), /job quality must declare its own permissions/);
  const persisted = mutated("        with:\n          persist-credentials: false\n", "");
  assert.match(hygieneFailures("ci.yml", persisted).join("\n"), /persist-credentials: false/);
  const broadTop = mutated("permissions:\n  contents: read\n\njobs:", "permissions: write-all\n\njobs:");
  assert.match(hygieneFailures("ci.yml", broadTop).join("\n"), /top-level permissions/);
});

test("release: each rule fails closed when broken", () => {
  const cases = [
    ["publish gets extra permission", ["    permissions:\n      contents: write\n", "    permissions:\n      contents: write\n      packages: write\n"], /publish permissions/],
    ["build can write contents", ["      contents: read\n      id-token: write", "      contents: write\n      id-token: write"], /build permissions/],
    ["publish without needs", ["    needs: [windows-release-build]\n", ""], /publish must need/],
    ["publish on dispatch", ["    if: startsWith(github.ref, 'refs/tags/v')\n    needs: [windows-release-build]", "    if: always()\n    needs: [windows-release-build]"], /tags only/],
    ["continue-on-error", ["      - name: Re-verify release assets\n", "      - name: Re-verify release assets\n        continue-on-error: true\n"], /continue-on-error/],
    ["signing step unguarded", ["        if: env.SIGNING_MODE == 'authenticode'\n        shell: pwsh\n        run: ./scripts/prepare-windows-signing.ps1", "        shell: pwsh\n        run: ./scripts/prepare-windows-signing.ps1"], /outside an authenticode guard/],
    ["mode not validated", ["run: ./scripts/check-signing-mode.ps1 -Mode $env:SIGNING_MODE", "run: echo skipped"], /validate the signing mode/],
    ["update key leaks to publish", ["      GH_TOKEN: ${{ github.token }}\n", "      GH_TOKEN: ${{ github.token }}\n      KEY: ${{ secrets.UPDATE_SIGNING_KEY }}\n"], /update signing key/],
    ["manifest before release", ["      - name: Create GitHub release\n", "      - name: Publish update manifest early\n        run: node scripts/publish-update-manifest.mjs\n\n      - name: Create GitHub release\n"], /manifest last|exactly once|before verification/],
    ["attestation verify removed", ["      - name: Verify build provenance attestations\n", "      - name: Skipped attestations\n"], /attestations/],
    ["old single job", ["  windows-release-build:", "  windows-release:\n    name: old\n  windows-release-build:"], /old single windows-release job/],
  ];
  for (const [name, [from, to], message] of cases) {
    assert.match(releaseFailures(mutated(from, to), scripts).join("\n"), message, name);
  }
});

test("release: helper scripts must enforce the mode", () => {
  assert.match(releaseFailures(ci, { ...scripts, verify: scripts.verify.replace(/NotSigned/g, "Valid") }).join("\n"), /verify-windows-release/);
  assert.match(releaseFailures(ci, { ...scripts, prepare: scripts.prepare.replace('SigningMode -ne "authenticode"', "false") }).join("\n"), /prepare-windows-signing/);
});
