// Publishes update.json + update.json.sig to the `updates` branch, which the app
// reads at a fixed raw.githubusercontent.com path. Runs LAST in the publish job,
// so clients never see a manifest whose installer is not yet a release asset.
// Uses the GitHub contents API through `gh` (argument arrays, no shell, no
// persisted git credentials).
//   node scripts/publish-update-manifest.mjs --dir release-assets --tag v1.2.3
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { versionFromTag } from "./release-assets.mjs";

export const UPDATES_BRANCH = "updates";

function option(name) {
  const index = process.argv.indexOf(`--${name}`);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

function gh(args, input) {
  return execFileSync("gh", args, { encoding: "utf8", input, stdio: ["pipe", "pipe", "inherit"] });
}

function existingSha(repository, path) {
  try {
    return JSON.parse(gh(["api", `repos/${repository}/contents/${path}?ref=${UPDATES_BRANCH}`])).sha;
  } catch {
    return null; // First publish: the file does not exist yet.
  }
}

function put(repository, path, bytes, message) {
  const body = { message, content: bytes.toString("base64"), branch: UPDATES_BRANCH };
  const sha = existingSha(repository, path);
  if (sha) body.sha = sha;
  gh(["api", "--method", "PUT", `repos/${repository}/contents/${path}`, "--input", "-"], JSON.stringify(body));
}

try {
  const repository = process.env.GITHUB_REPOSITORY;
  if (!repository) throw new Error("GITHUB_REPOSITORY is not set");
  const dir = resolve(option("dir") ?? "release-assets");
  const version = versionFromTag(option("tag"));
  // Signature first: a client that fetches between the two writes sees the new
  // signature with the old manifest, which fails verification (no update) — never
  // an unverified manifest.
  put(repository, "update.json.sig", readFileSync(join(dir, "update.json.sig")), `Update signature for ${version}`);
  put(repository, "update.json", readFileSync(join(dir, "update.json")), `Update manifest for ${version}`);
  console.log(`Published the update manifest for ${version} to the ${UPDATES_BRANCH} branch.`);
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
