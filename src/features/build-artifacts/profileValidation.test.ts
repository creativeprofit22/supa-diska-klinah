import storage from "../../../src-tauri/crates/windows-platform/src/cleanup/storage.rs?raw";
import { describe, expect, it } from "vitest";
import * as validation from "./profileValidation";
import { buildArtifactsStrings } from "./strings";

describe("build profile validation parity", () => {
  it("matches the authoritative Rust byte and count limits", () => {
    for (const name of ["MAX_PROFILE_ARGUMENTS", "MAX_PROFILE_ARTIFACTS", "MAX_ARGUMENT_BYTES", "MAX_STRING_BYTES", "MAX_PROJECT_PATH_BYTES"] as const) {
      const value = storage.match(new RegExp(`const ${name}: usize = ([\\d_]+);`));
      expect(value, name).not.toBeNull();
      expect(validation[name], name).toBe(Number(value![1].replaceAll("_", "")));
    }
  });

  it.each([
    ["Target/app", "target\\app"],
    ["target//app/", "target/./app"],
    ["target", "target/app"],
  ])("rejects normalized duplicates and overlaps: %s, %s", (first, second) => {
    expect(validation.artifactPathErrors([first, second]).every(Boolean)).toBe(true);
  });

  it("compares whole components and folds only ASCII, like Rust", () => {
    expect(validation.artifactPathErrors(["target/app", "target/application", "Ä/app", "ä/app"])).toEqual([undefined, undefined, undefined, undefined]);
  });

  it.each(["../target", "./target", "C:target", "C:\\target", "/target", "target/../app"])("rejects invalid relative path %s", (path) => {
    expect(validation.artifactPathErrors([path])[0]).toBeTruthy();
  });

  it("allows empty arguments but rejects nulls and label control characters", () => {
    expect(validation.textError("", 4096)).toBeUndefined();
    expect(validation.textError("a\0", 4096)).toBeTruthy();
    expect(validation.textError("a\u0085", 1024, true)).toBeTruthy();
  });

  it("defaults to English messages and uses the supplied catalog", () => {
    const es = buildArtifactsStrings.es419.validation;
    expect(validation.textError("a\0", 4096)).toBe("Null characters are not allowed.");
    expect(validation.textError("", 1024, true, es)).toBe(es.enterLabel);
    expect(validation.textError("ab", 1, false, es)).toBe(es.tooManyBytes(1));
    expect(validation.artifactPathErrors(["../target"])).toEqual([
      "Enter a relative path without a root, drive prefix, or parent components.",
    ]);
    expect(validation.artifactPathErrors(["target", "target/app"], es)).toEqual([es.overlappingPath, es.overlappingPath]);
    expect(validation.artifactPathErrors(["C:\\target"], es)).toEqual([es.invalidRelativePath]);
  });
});
