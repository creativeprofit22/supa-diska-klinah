import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { generateKeyPairSync } from "node:crypto";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { privateKeyPathAllowed, rawPublicKeyHex, signPack, verifyPack } from "./rule-pack.mjs";

const baseline = "src-tauri/crates/windows-platform/src/protection/baseline";
const repoRoot = resolve(import.meta.dirname, "..");
const protection = join(repoRoot, "src-tauri/crates/windows-platform/src/protection");

const keyPathCases = [
  { name: "sibling folder sharing the fixtures prefix", path: join(protection, "fixtures-release", "k.pem"), allowed: false },
  { name: "repository root", path: join(repoRoot, "k.pem"), allowed: false },
  { name: "other file inside fixtures", path: join(protection, "fixtures", "release.pem"), allowed: false },
  { name: "committed test key", path: join(protection, "fixtures", "test-rule-pack.pem"), allowed: true },
  { name: "outside the repository", path: join(tmpdir(), "k.pem"), allowed: true },
];

for (const { name, path, allowed } of keyPathCases) {
  test(`private key path: ${name} -> ${allowed}`, () => {
    assert.equal(privateKeyPathAllowed(path), allowed);
  });
}

test("embedded baseline verifies against the compiled public key", () => {
  const ok = verifyPack(
    readFileSync("src-tauri/keys/rule-pack.pub", "utf8"),
    readFileSync(`${baseline}/pack.json`),
    readFileSync(`${baseline}/pack.sig`, "utf8"),
  );
  assert.equal(ok, true);
});

test("sign and verify round-trip; tampering and other keys fail", () => {
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  const other = generateKeyPairSync("ed25519");
  const pem = privateKey.export({ format: "pem", type: "pkcs8" });
  const bytes = Buffer.from('{"format":1}');
  const sig = signPack(pem, bytes);
  assert.match(sig, /^[0-9a-f]{128}$/);
  assert.equal(verifyPack(rawPublicKeyHex(publicKey), bytes, sig), true);
  assert.equal(verifyPack(rawPublicKeyHex(publicKey), Buffer.from('{"format":2}'), sig), false);
  assert.equal(verifyPack(rawPublicKeyHex(other.publicKey), bytes, sig), false);
  assert.equal(verifyPack(rawPublicKeyHex(publicKey), bytes, sig.slice(2)), false);
});
