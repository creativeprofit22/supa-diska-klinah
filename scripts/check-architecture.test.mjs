import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { cpSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve, sep } from "node:path";
import test from "node:test";

const root = resolve(import.meta.dirname, "..");
const fixture = "src-tauri/crates/windows-platform/tests/fixtures/native-process-tree.rs";

test("architecture scanner boundaries", async (t) => {
  const sandbox = mkdtempSync(resolve(tmpdir(), "architecture-regression-"));
  t.after(() => rmSync(sandbox, { recursive: true, force: true }));
  const generated = new Set(
    ["target", "binaries", "gen"].map((name) => resolve(root, "src-tauri", name)),
  );
  // Use controlled frontend imports; the full architecture command checks the application.
  mkdirSync(resolve(sandbox, "src"));
  for (const path of ["scripts/check-architecture.mjs", "src-tauri"]) {
    const destination = resolve(sandbox, path);
    mkdirSync(dirname(destination), { recursive: true });
    cpSync(resolve(root, path), destination, {
      recursive: true,
      filter: (source) => !generated.has(resolve(source)),
    });
  }
  function scan() {
    const result = spawnSync(process.execPath, ["scripts/check-architecture.mjs"], {
      cwd: sandbox,
      encoding: "utf8",
      timeout: 30_000,
      shell: false,
    });
    assert.ifError(result.error);
    return result;
  }

  await t.test("permits the real cancellation fixture", () => {
    const result = scan();
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /Architecture boundaries verified/);
  });

  for (const path of [
    "src-tauri/crates/windows-platform/src/cleanup/native-process-tree.rs",
    "src-tauri/crates/windows-platform/tests/fixtures/native-process-tree-copy.rs",
    "src-tauri/crates/cleanup-core/tests/fixtures/native-process-tree.rs",
  ]) {
    await t.test(`rejects the same Command::new source at ${path}`, () => {
      const destination = resolve(sandbox, path);
      cpSync(resolve(sandbox, fixture), destination);
      try {
        const result = scan();
        assert.equal(result.status, 1, result.stdout);
        assert.ok(
          result.stderr.split(sep).join("/").includes(
            `${path} contains forbidden runtime process execution`,
          ),
          result.stderr,
        );
      } finally {
        rmSync(destination);
      }
    });
  }

  const importer = resolve(sandbox, "src/features/build-artifacts/profileValidation.test.ts");
  const otherFeature = resolve(sandbox, "src/features/other/value.ts");
  mkdirSync(dirname(importer), { recursive: true });
  mkdirSync(dirname(otherFeature), { recursive: true });
  writeFileSync(otherFeature, "export default 1;\n");
  for (const [name, specifier, status, message] of [
    ["resolves an existing raw Rust target", "../../../src-tauri/crates/windows-platform/src/cleanup/storage.rs?raw", 0, "Architecture boundaries verified."],
    ["rejects a missing raw target", "./missing.rs?raw", 1, "imports missing local module ./missing.rs?raw"],
    ["rejects a feature crossing with raw", "../other/value.ts?raw", 1, "crosses its feature boundary"],
  ]) {
    await t.test(name, () => {
      writeFileSync(importer, `import content from "${specifier}";\n`);
      const result = scan();
      assert.equal(result.status, status, result.stderr);
      assert.ok(`${result.stdout}${result.stderr}`.includes(message), result.stderr);
    });
  }
  rmSync(importer);

  const cleanupApi = resolve(sandbox, "src/features/cleanup/api");
  mkdirSync(cleanupApi, { recursive: true });
  for (const name of ["previewCleanup", "unapproved"]) {
    writeFileSync(resolve(cleanupApi, `${name}.ts`), "export const listProjectRoots = () => [];\n");
  }
  for (const [source, target, status] of [
    ["useProjectRoots", "previewCleanup", 0],
    ["useProjectRoots", "unapproved", 1],
    ["unapprovedNeighbor", "previewCleanup", 1],
  ]) {
    await t.test(`bridge ${source} -> ${target} ${status === 0 ? "passes" : "fails"}`, () => {
      const path = resolve(sandbox, `src/features/build-artifacts/${source}.ts`);
      writeFileSync(path, `import { listProjectRoots } from "../cleanup/api/${target}";\n`);
      try {
        const result = scan();
        assert.equal(result.status, status, result.stderr);
        if (status === 0) {
          assert.match(result.stdout, /Architecture boundaries verified/);
        } else {
          assert.ok(result.stderr.split(sep).join("/").includes(
            `src/features/build-artifacts/${source}.ts crosses its feature boundary`,
          ), result.stderr);
        }
      } finally {
        rmSync(path);
      }
    });
  }
});
