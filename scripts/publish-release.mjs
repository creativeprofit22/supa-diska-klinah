// Creates the GitHub release from re-verified assets. Runs only in the
// tag-triggered publish job, after verify-release-assets.mjs and attestation checks.
//   node scripts/publish-release.mjs --dir release-assets --tag v1.2.3 --signing-mode unsigned
import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { releaseText, verifyReleaseDir } from "./release-assets.mjs";

const REPO_ROOT = resolve(import.meta.dirname, "..");

function option(name) {
  const index = process.argv.indexOf(`--${name}`);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

try {
  const dir = resolve(option("dir") ?? "release-assets");
  const tag = option("tag");
  const mode = option("signing-mode");
  const repository = process.env.GITHUB_REPOSITORY;
  if (!repository) throw new Error("GITHUB_REPOSITORY is not set");
  // Verify again immediately before publishing (no gap between check and use).
  verifyReleaseDir({
    dir,
    tag,
    mode,
    publicKeyHex: readFileSync(resolve(REPO_ROOT, "src-tauri/keys/update.pub"), "utf8"),
    releaseDoc: readFileSync(resolve(REPO_ROOT, "docs/release.md"), "utf8"),
  });
  const { title, notes } = releaseText({ tag, mode, repository });
  const notesFile = join(process.env.RUNNER_TEMP ?? dir, "release-notes.md");
  writeFileSync(notesFile, notes);
  const assets = readdirSync(dir).sort().map((name) => join(dir, name));
  // Argument array; no shell. --verify-tag refuses to create a missing tag.
  execFileSync("gh", ["release", "create", tag, "--verify-tag", "--title", title, "--notes-file", notesFile, ...assets], {
    stdio: "inherit",
  });
  console.log(`Published ${title}.`);
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
