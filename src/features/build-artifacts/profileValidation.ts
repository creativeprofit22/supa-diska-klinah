// Mirrors src-tauri/crates/windows-platform/src/cleanup/storage.rs.
// profileValidation.test.ts checks constant parity; Rust remains authoritative.
export const MAX_PROFILE_ARGUMENTS = 64;
export const MAX_PROFILE_ARTIFACTS = 16;
export const MAX_ARGUMENT_BYTES = 4096;
export const MAX_STRING_BYTES = 1024;
export const MAX_PROJECT_PATH_BYTES = 4096;

const encoder = new TextEncoder();

export function textError(value: string, maximum: number, label = false): string | undefined {
  if (encoder.encode(value).length > maximum) return `Use at most ${maximum} UTF-8 bytes.`;
  if (label && !value) return "Enter a label.";
  if ([...value].some((character) => {
    const code = character.codePointAt(0)!;
    return label ? code <= 31 || (code >= 127 && code <= 159) : code === 0;
  })) return label ? "Control characters are not allowed." : "Null characters are not allowed.";
}

// Windows Path components: separators collapse, internal '.' is ignored,
// but leading '.', parent components, roots and drive prefixes are rejected.
// Match Rust's ASCII-only case folding, not Unicode lowercasing.
function normalizedArtifactPath(value: string): string | undefined {
  if (!value || textError(value, MAX_PROJECT_PATH_BYTES) || /^[\\/]|^[a-z]:/i.test(value)) return;
  const parts = value.replaceAll("\\", "/").split("/").filter(Boolean);
  if (parts[0] === "." || parts.includes("..")) return;
  return parts.filter((part) => part !== ".").join("/").replace(/[A-Z]/g, (letter) => letter.toLowerCase()) || undefined;
}

export function artifactPathErrors(paths: string[]): (string | undefined)[] {
  const normalized = paths.map(normalizedArtifactPath);
  return normalized.map((path, index) => {
    const byteError = textError(paths[index], MAX_PROJECT_PATH_BYTES);
    if (byteError) return byteError;
    if (!path) return "Enter a relative path without a root, drive prefix, or parent components.";
    if (normalized.some((other, otherIndex) => otherIndex !== index && other &&
      (path === other || path.startsWith(`${other}/`) || other.startsWith(`${path}/`)))) {
      return "Artifact paths must not duplicate or overlap another path.";
    }
  });
}
