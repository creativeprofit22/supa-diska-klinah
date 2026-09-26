// One-shot generator for the app-update Ed25519 key (separate from the
// rule-pack key).
//
//   node scripts/generate-update-key.mjs [--private-out <path outside the repo>]
//
// Writes the public key to src-tauri/keys/update.pub, but only while that
// file still holds the `unconfigured` marker, so a live key is never replaced
// by accident (rotation is a deliberate manual step, see docs/updates.md).
// The private key (PKCS#8 PEM) is printed once for pasting into the
// `UPDATE_SIGNING_KEY` secret of the `windows-release` environment, or written
// with exclusive-create to --private-out, which must be outside the repository.
// It is never written inside the repository.

import { generateKeyPairSync } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { privateKeyPathAllowed, rawPublicKeyHex } from "./rule-pack.mjs";

const REPO_ROOT = resolve(import.meta.dirname, "..");
const TEST_PRIVATE_KEY = resolve(REPO_ROOT, "src-tauri/crates/windows-platform/src/protection/fixtures/test-rule-pack.pem");

export function generate({ publicPath, privateOut, rulePackPublicPath }) {
  const current = readFileSync(publicPath, "utf8").trim();
  if (current !== "unconfigured") {
    throw new Error(`${publicPath} already holds a key; rotate it deliberately (docs/updates.md), not with this script`);
  }
  if (privateOut && (!privateKeyPathAllowed(privateOut) || resolve(privateOut) === TEST_PRIVATE_KEY)) {
    throw new Error("refusing to write the update private key inside the repository");
  }
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  const publicHex = rawPublicKeyHex(publicKey);
  if (publicHex === readFileSync(rulePackPublicPath, "utf8").trim()) throw new Error("generated key collides with the rule-pack key");
  const pem = privateKey.export({ format: "pem", type: "pkcs8" });
  if (privateOut) writeFileSync(privateOut, pem, { flag: "wx", mode: 0o600 });
  writeFileSync(publicPath, `${publicHex}\n`);
  return { publicHex, pem };
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  try {
    const args = process.argv.slice(2);
    const index = args.indexOf("--private-out");
    const privateOut = index >= 0 ? args[index + 1] : undefined;
    if (index >= 0 && !privateOut) throw new Error("--private-out needs a path");
    const { publicHex, pem } = generate({
      publicPath: resolve(REPO_ROOT, "src-tauri/keys/update.pub"),
      privateOut,
      rulePackPublicPath: resolve(REPO_ROOT, "src-tauri/keys/rule-pack.pub"),
    });
    console.log(`Public key written to src-tauri/keys/update.pub: ${publicHex}`);
    if (privateOut) {
      console.log(`Private key written to ${privateOut}. Store it in the UPDATE_SIGNING_KEY secret, then move the file offline.`);
    } else {
      console.log("Private key (shown once; paste it into the UPDATE_SIGNING_KEY secret of the windows-release environment):");
      console.log(pem);
    }
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
}
