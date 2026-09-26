import assert from "node:assert/strict";
import { generateKeyPairSync, createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { generate } from "./generate-update-key.mjs";
import { rawPublicKeyHex, verifyPack } from "./rule-pack.mjs";
import { buildManifest, installerName, run } from "./sign-update-manifest.mjs";

const NOW = 1_790_000_000;
const SHA = "a".repeat(64);
const THUMB = "0123456789ABCDEF0123456789ABCDEF01234567";

function keys() {
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  return { pem: privateKey.export({ format: "pem", type: "pkcs8" }), publicHex: rawPublicKeyHex(publicKey) };
}

test("builds the exact schema the app verifies", () => {
  const bytes = buildManifest({ version: "0.2.0", minimumVersion: "0.1.0", size: 10, sha256: SHA, notBefore: NOW, validDays: 90, signingMode: "unsigned" });
  assert.equal(
    bytes.toString(),
    `{"format":1,"version":"0.2.0","minimumVersion":"0.1.0","installer":{"name":"Supa-Diska-Klinah_0.2.0_x64-setup.exe","size":10,"sha256":"${SHA}"},"notBefore":${NOW},"expires":${NOW + 90 * 86400},"signing":"none"}`,
  );
  const signed = JSON.parse(buildManifest({ version: "1.0.0", minimumVersion: "0.1.0", size: 10, sha256: SHA, notBefore: NOW, validDays: 30, signingMode: "authenticode", thumbprint: THUMB }).toString());
  assert.equal(signed.signing, "authenticode");
  assert.equal(signed.signerThumbprint, THUMB);
});

test("rejects inconsistent or unsafe manifests", () => {
  const base = { version: "0.2.0", minimumVersion: "0.1.0", size: 10, sha256: SHA, notBefore: NOW, validDays: 90, signingMode: "unsigned" };
  for (const [change, message] of [
    [{ signingMode: undefined }, /signing mode/],
    [{ signingMode: "maybe" }, /signing mode/],
    [{ signingMode: "unsigned", thumbprint: THUMB }, /must not carry/],
    [{ signingMode: "authenticode" }, /thumbprint/],
    [{ signingMode: "authenticode", thumbprint: THUMB.toLowerCase() }, /thumbprint/],
    [{ validDays: 91 }, /valid days/],
    [{ validDays: 0 }, /valid days/],
    [{ version: "0.2.0-beta" }, /version/],
    [{ minimumVersion: "0.3.0" }, /minimum/],
    [{ size: 0 }, /size/],
    [{ size: 512 * 1024 * 1024 + 1 }, /size/],
    [{ sha256: SHA.toUpperCase() }, /sha256/],
  ]) {
    assert.throws(() => buildManifest({ ...base, ...change }), message, JSON.stringify(change));
  }
});

function setup(version = "0.2.0") {
  const dir = mkdtempSync(join(tmpdir(), "sdk-update-"));
  const installer = join(dir, installerName(version));
  writeFileSync(installer, Buffer.from("MZ fake installer"));
  return { dir, installer };
}

test("signs, self-verifies and writes both files", () => {
  const { pem, publicHex } = keys();
  const { dir, installer } = setup();
  try {
    const publicKey = join(dir, "update.pub");
    writeFileSync(publicKey, `${publicHex}\n`);
    run(["--installer", installer, "--version", "0.2.0", "--signing-mode", "unsigned", "--out-dir", dir, "--public-key", publicKey], { UPDATE_SIGNING_KEY: pem }, NOW);
    const bytes = readFileSync(join(dir, "update.json"));
    const manifest = JSON.parse(bytes.toString());
    assert.equal(manifest.installer.sha256, createHash("sha256").update(readFileSync(installer)).digest("hex"));
    assert.equal(manifest.installer.size, readFileSync(installer).length);
    assert.equal(verifyPack(publicHex, bytes, readFileSync(join(dir, "update.json.sig"), "utf8")), true);
    // Never overwrites an existing manifest.
    assert.throws(() => run(["--installer", installer, "--version", "0.2.0", "--signing-mode", "unsigned", "--out-dir", dir, "--public-key", publicKey], { UPDATE_SIGNING_KEY: pem }, NOW), /EEXIST/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("fails closed on missing key, key mismatch, unconfigured key and wrong installer name", () => {
  const { pem, publicHex } = keys();
  const other = keys();
  const { dir, installer } = setup();
  try {
    const publicKey = join(dir, "update.pub");
    writeFileSync(publicKey, `${publicHex}\n`);
    const args = ["--installer", installer, "--version", "0.2.0", "--signing-mode", "unsigned", "--out-dir", dir, "--public-key", publicKey];
    assert.throws(() => run(args, {}, NOW), /UPDATE_SIGNING_KEY/);
    assert.throws(() => run(args, { UPDATE_SIGNING_KEY: other.pem }, NOW), /does not match/);
    writeFileSync(publicKey, "unconfigured\n");
    assert.throws(() => run(args, { UPDATE_SIGNING_KEY: pem }, NOW), /unconfigured/);
    writeFileSync(publicKey, `${publicHex}\n`);
    assert.throws(() => run(args.map((a) => (a === "0.2.0" ? "0.3.0" : a)), { UPDATE_SIGNING_KEY: pem }, NOW), /must be named/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("key generator only replaces the unconfigured marker and keeps private keys out of the repo", () => {
  const dir = mkdtempSync(join(tmpdir(), "sdk-keygen-"));
  try {
    const publicPath = join(dir, "update.pub");
    const rulePackPublicPath = join(dir, "rule-pack.pub");
    writeFileSync(rulePackPublicPath, `${"b".repeat(64)}\n`);
    writeFileSync(publicPath, "unconfigured\n");
    const privateOut = join(dir, "update.pem");
    const { publicHex } = generate({ publicPath, privateOut, rulePackPublicPath });
    assert.match(readFileSync(publicPath, "utf8"), /^[0-9a-f]{64}\n$/);
    assert.equal(readFileSync(publicPath, "utf8").trim(), publicHex);
    assert.match(readFileSync(privateOut, "utf8"), /BEGIN PRIVATE KEY/);
    assert.throws(() => generate({ publicPath, rulePackPublicPath }), /already holds a key/);
    writeFileSync(publicPath, "unconfigured\n");
    assert.throws(() => generate({ publicPath, privateOut: "scripts/leak.pem", rulePackPublicPath }), /inside the repository/);
    assert.equal(readFileSync(publicPath, "utf8"), "unconfigured\n");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
