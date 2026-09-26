import assert from "node:assert/strict";
import { generateKeyPairSync } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  assertSigningMode,
  declaredSigningMode,
  parseSums,
  releaseText,
  sha256,
  verifyReleaseDir,
  versionFromTag,
} from "./release-assets.mjs";
import { rawPublicKeyHex, signPack } from "./rule-pack.mjs";
import { buildManifest, installerName } from "./sign-update-manifest.mjs";

const NOW = 1_790_000_000;
const DOC = (mode) => `# Release\n\nCurrent signing mode: ${mode}\n`;

function stage({ mode = "unsigned", version = "0.2.0", mutate } = {}) {
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  const pem = privateKey.export({ format: "pem", type: "pkcs8" });
  const dir = mkdtempSync(join(tmpdir(), "sdk-release-"));
  const installer = Buffer.from(`MZ installer ${version}`);
  writeFileSync(join(dir, installerName(version)), installer);
  writeFileSync(join(dir, "dependency-inventory.json"), "{}\n");
  const manifest = buildManifest({
    version, minimumVersion: "0.1.0", size: installer.length, sha256: sha256(installer), notBefore: NOW, validDays: 90,
    signingMode: mode, thumbprint: mode === "authenticode" ? "0123456789ABCDEF0123456789ABCDEF01234567" : undefined,
  });
  writeFileSync(join(dir, "update.json"), manifest);
  writeFileSync(join(dir, "update.json.sig"), `${signPack(pem, manifest)}\n`);
  mutate?.(dir);
  const sums = [installerName(version), "dependency-inventory.json", "update.json", "update.json.sig"]
    .map((name) => `${sha256(readFileSync(join(dir, name)))}  ${name}`).join("\n");
  writeFileSync(join(dir, "SHA256SUMS"), `${sums}\n`);
  return { dir, publicKeyHex: rawPublicKeyHex(publicKey) };
}

function check(options, overrides = {}) {
  return verifyReleaseDir({ tag: "v0.2.0", mode: "unsigned", releaseDoc: DOC("unsigned"), now: NOW, ...options, ...overrides });
}

test("signing mode must be exact and match the docs marker", () => {
  assert.equal(declaredSigningMode(DOC("unsigned")), "unsigned");
  assert.equal(declaredSigningMode("Current signing mode: `authenticode`"), "authenticode");
  assert.equal(declaredSigningMode("no marker"), null);
  assert.equal(assertSigningMode("unsigned", DOC("unsigned")), "unsigned");
  for (const mode of [undefined, "", "Unsigned", "none", "signed"]) {
    assert.throws(() => assertSigningMode(mode, DOC("unsigned")), /must be/);
  }
  assert.throws(() => assertSigningMode("authenticode", DOC("unsigned")), /declares "unsigned"/);
  assert.throws(() => assertSigningMode("unsigned", "nothing"), /declares "nothing"/);
});

test("tags map to strict versions", () => {
  assert.equal(versionFromTag("v1.2.3"), "1.2.3");
  for (const tag of ["1.2.3", "v1.2", "v1.2.3-rc1", "", undefined]) assert.throws(() => versionFromTag(tag));
});

test("accepts a complete, consistent release", () => {
  const staged = stage();
  try {
    assert.equal(check(staged).signing, "none");
  } finally {
    rmSync(staged.dir, { recursive: true, force: true });
  }
});

test("fails closed on every tampering or mismatch", () => {
  const cases = [
    ["wrong tag", {}, { tag: "v0.3.0" }, /exactly|must be exactly/],
    ["mode vs docs", {}, { mode: "authenticode" }, /declares/],
    ["manifest signing vs mode", { mode: "authenticode" }, { mode: "unsigned" }, /signing is "authenticode"/],
    ["tampered installer", { mutate: (d) => writeFileSync(join(d, installerName("0.2.0")), "evil") }, {}, /size or SHA-256/],
    ["tampered manifest", { mutate: (d) => writeFileSync(join(d, "update.json"), readFileSync(join(d, "update.json"), "utf8").replace("0.1.0", "0.0.1")) }, {}, /signature/],
    ["extra file", { mutate: (d) => writeFileSync(join(d, "extra.txt"), "x") }, {}, /exactly/],
    ["unconfigured key", {}, { publicKeyHex: "unconfigured\n" }, /unconfigured/],
    ["expired", {}, { now: NOW + 91 * 86400 }, /not currently valid/],
  ];
  for (const [name, stageOptions, overrides, message] of cases) {
    const staged = stage(stageOptions);
    try {
      assert.throws(() => check(staged, overrides), message, name);
    } finally {
      rmSync(staged.dir, { recursive: true, force: true });
    }
  }
});

test("checksum file is strict", () => {
  const staged = stage();
  try {
    writeFileSync(join(staged.dir, "SHA256SUMS"), readFileSync(join(staged.dir, "SHA256SUMS"), "utf8").replace(/^./, "0"));
    assert.throws(() => check(staged), /checksum mismatch|malformed/);
  } finally {
    rmSync(staged.dir, { recursive: true, force: true });
  }
  assert.throws(() => parseSums("abc  file"), /malformed/);
  assert.throws(() => parseSums(`${"a".repeat(64)}  a\n${"b".repeat(64)}  a`), /malformed/);
  assert.throws(() => parseSums(`${"a".repeat(64)}  ../x`), /malformed/);
});

test("unsigned releases are labelled and explain verification", () => {
  const unsigned = releaseText({ tag: "v0.2.0", mode: "unsigned", repository: "o/r" });
  assert.equal(unsigned.title, "Supa Diska Klinah 0.2.0 (unsigned)");
  assert.match(unsigned.notes, /not code-signed/);
  assert.match(unsigned.notes, /SmartScreen/);
  assert.match(unsigned.notes, /gh attestation verify Supa-Diska-Klinah_0\.2\.0_x64-setup\.exe --repo o\/r/);
  const signed = releaseText({ tag: "v0.2.0", mode: "authenticode", repository: "o/r" });
  assert.equal(signed.title, "Supa Diska Klinah 0.2.0");
  assert.doesNotMatch(signed.notes, /not code-signed/);
  assert.match(signed.notes, /Get-FileHash/);
});
