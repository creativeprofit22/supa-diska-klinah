// Rule-pack key generation, signing and verification (ADR 0003).
//
// Uses node:crypto Ed25519 only; no dependency is added.
//
//   node scripts/rule-pack.mjs keygen --private <file.pem> --public <file.pub>
//   node scripts/rule-pack.mjs sign   --private <file.pem> --pack <pack.json> [--out <pack.sig>]
//   node scripts/rule-pack.mjs verify --public <file.pub> --pack <pack.json> [--sig <pack.sig>]
//
// Public keys are 64 hex characters of the raw Ed25519 key. Signatures are
// 128 hex characters over the exact bytes of pack.json. Private keys are
// PKCS#8 PEM and are written with exclusive-create so nothing is overwritten.
// Keep release private keys outside the repository.

import { createPrivateKey, createPublicKey, generateKeyPairSync, sign, verify } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { resolve, relative, isAbsolute, sep } from "node:path";
import { pathToFileURL } from "node:url";

const SPKI_ED25519_PREFIX = Buffer.from("302a300506032b6570032100", "hex");
const REPO_ROOT = resolve(import.meta.dirname, "..");
const TEST_PRIVATE_KEY = resolve(REPO_ROOT, "src-tauri/crates/windows-platform/src/protection/fixtures/test-rule-pack.pem");

export function rawPublicKeyHex(publicKey) {
  const der = publicKey.export({ format: "der", type: "spki" });
  if (der.length !== 44 || !der.subarray(0, 12).equals(SPKI_ED25519_PREFIX)) {
    throw new Error("not an Ed25519 public key");
  }
  return der.subarray(12).toString("hex");
}

export function publicKeyFromHex(text) {
  const hex = text.replace(/\r?\n$/, "");
  if (!/^[0-9a-f]{64}$/.test(hex)) throw new Error("public key must be 64 lowercase hex characters");
  return createPublicKey({
    key: Buffer.concat([SPKI_ED25519_PREFIX, Buffer.from(hex, "hex")]),
    format: "der",
    type: "spki",
  });
}

export function signPack(privatePem, packBytes) {
  const key = createPrivateKey(privatePem);
  if (key.asymmetricKeyType !== "ed25519") throw new Error("private key is not Ed25519");
  return sign(null, packBytes, key).toString("hex");
}

export function verifyPack(publicHex, packBytes, signatureText) {
  const sig = signatureText.replace(/\r?\n$/, "");
  if (!/^[0-9a-fA-F]{128}$/.test(sig)) return false;
  return verify(null, packBytes, publicKeyFromHex(publicHex), Buffer.from(sig, "hex"));
}

function isInsideRepo(path) {
  const rel = relative(REPO_ROOT, resolve(path));
  return rel !== ".." && !rel.startsWith(`..${sep}`) && !isAbsolute(rel);
}

// Private keys must live outside the repository; the only exception is the
// committed test key file itself.
export function privateKeyPathAllowed(path) {
  const target = resolve(path);
  return !isInsideRepo(target) || target === TEST_PRIVATE_KEY;
}

function parseArgs(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 2) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (!flag?.startsWith("--") || value === undefined) throw new Error(`bad argument near ${flag}`);
    options[flag.slice(2)] = value;
  }
  return options;
}

function main([command, ...rest]) {
  const options = parseArgs(rest);
  switch (command) {
    case "keygen": {
      if (!options.private || !options.public) throw new Error("keygen needs --private and --public");
      if (!privateKeyPathAllowed(options.private)) {
        throw new Error("refusing to write a private key inside the repository (only the committed test key is excepted)");
      }
      const { privateKey, publicKey } = generateKeyPairSync("ed25519");
      writeFileSync(options.private, privateKey.export({ format: "pem", type: "pkcs8" }), { flag: "wx", mode: 0o600 });
      writeFileSync(options.public, `${rawPublicKeyHex(publicKey)}\n`, { flag: "wx" });
      console.log(`Wrote ${options.public}. Keep ${options.private} offline.`);
      return 0;
    }
    case "sign": {
      if (!options.private || !options.pack) throw new Error("sign needs --private and --pack");
      const signature = signPack(readFileSync(options.private), readFileSync(options.pack));
      writeFileSync(options.out ?? resolve(options.pack, "..", "pack.sig"), `${signature}\n`);
      console.log("Signed.");
      return 0;
    }
    case "verify": {
      if (!options.public || !options.pack) throw new Error("verify needs --public and --pack");
      const ok = verifyPack(
        readFileSync(options.public, "utf8"),
        readFileSync(options.pack),
        readFileSync(options.sig ?? resolve(options.pack, "..", "pack.sig"), "utf8"),
      );
      console.log(ok ? "Signature valid." : "Signature INVALID.");
      return ok ? 0 : 1;
    }
    default:
      console.error("usage: rule-pack.mjs keygen|sign|verify …");
      return 2;
  }
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  try {
    process.exitCode = main(process.argv.slice(2));
  } catch (error) {
    console.error(`rule-pack: ${error.message}`);
    process.exitCode = 1;
  }
}
