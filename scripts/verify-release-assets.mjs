// Publish-side re-verification of the staged release assets (see release-assets.mjs).
//   node scripts/verify-release-assets.mjs --dir release-assets --tag v1.2.3 --signing-mode unsigned
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { verifyReleaseDir } from "./release-assets.mjs";

const REPO_ROOT = resolve(import.meta.dirname, "..");

function option(name) {
  const index = process.argv.indexOf(`--${name}`);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

try {
  const manifest = verifyReleaseDir({
    dir: resolve(option("dir") ?? "release-assets"),
    tag: option("tag"),
    mode: option("signing-mode"),
    publicKeyHex: readFileSync(resolve(REPO_ROOT, "src-tauri/keys/update.pub"), "utf8"),
    releaseDoc: readFileSync(resolve(REPO_ROOT, "docs/release.md"), "utf8"),
  });
  console.log(`Release assets for ${manifest.version} verified (${manifest.signing}).`);
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
