// Guards the local build strategy measured in docs/performance.md:
// - dev profiles keep Cargo incremental compilation (never `incremental = false`);
// - no committed Cargo config forces a rustc wrapper (sccache) onto local builds;
// - no script runs `cargo clean` automatically (artifact budgets forbid it).
import { readdir, readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_EXTENSIONS = new Set([".mjs", ".js", ".cjs", ".ps1", ".psm1", ".sh", ".cmd", ".bat"]);
const SKIP_DIRECTORIES = new Set(["node_modules", "target", "dist", ".git", ".gg"]);

/** Returns the TOML section header each line belongs to (minimal, comment-aware). */
function sectionedLines(text) {
  const lines = [];
  let section = "";
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.replace(/\s+#.*$/, "").replace(/^#.*$/, "").trim();
    const header = /^\[{1,2}([^\]]+)\]{1,2}$/.exec(line);
    if (header?.[1] !== undefined) {
      section = header[1].trim().replaceAll(/\s+/g, "");
      continue;
    }
    if (line !== "") lines.push({ section, line });
  }
  return lines;
}

/**
 * @param {string} file display path
 * @param {string} text TOML text of a Cargo.toml or .cargo/config(.toml)
 * @returns {string[]} violations
 */
export function checkCargoToml(file, text) {
  const violations = [];
  for (const { section, line } of sectionedLines(text)) {
    const isDevProfile = /^profile\.(dev|test)(\.|$)/.test(section);
    if (isDevProfile && /^incremental\s*=\s*false\b/.test(line)) {
      violations.push(`${file}: [${section}] disables incremental builds for local development`);
    }
    if (section === "build" && /^(rustc-wrapper|rustc-workspace-wrapper)\s*=/.test(line)) {
      violations.push(`${file}: [build] sets a rustc wrapper; sccache is CI/clean-build only`);
    }
    if (section === "build" && /^incremental\s*=\s*false\b/.test(line)) {
      violations.push(`${file}: [build] disables incremental builds for local development`);
    }
    if (section === "env" && /^(RUSTC_WRAPPER|CARGO_INCREMENTAL)\s*=/.test(line)) {
      violations.push(`${file}: [env] overrides ${line.split("=")[0]?.trim()} for local builds`);
    }
  }
  return violations;
}

/**
 * @param {string} file display path
 * @param {string} text script source
 * @returns {string[]} violations
 */
export function checkScript(file, text) {
  const violations = [];
  // Blank block comments but keep newlines so reported line numbers stay accurate.
  const blank = (block) => block.replaceAll(/[^\n]/g, " ");
  const code = text.replaceAll(/<#[\s\S]*?#>/g, blank).replaceAll(/\/\*[\s\S]*?\*\//g, blank);
  code.split(/\r?\n/).forEach((line, index) => {
    const statement = line.replace(/^\s*(#|\/\/).*$/, "");
    if (CARGO_CLEAN.some((pattern) => pattern.test(statement))) {
      violations.push(`${file}:${index + 1}: runs \`cargo clean\` automatically`);
    }
  });
  return violations;
}

const CARGO_CLEAN = [
  /(?<![`\w])cargo(?:\.exe)?["']?\s+clean\b/, // cargo clean / & "cargo" clean (not `cargo clean` prose)
  /\bcargo(?:\.exe)?["']\s*,\s*\[\s*["']clean["']/, // spawn("cargo", ["clean"])
  /["']?cargo(?:\.exe)?["']?\s+-(?:ArgumentList|Arguments)\s+@?\(?\s*["']clean["']/i, // PowerShell wrappers
];

async function* walk(directory) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      if (!SKIP_DIRECTORIES.has(entry.name)) yield* walk(path.join(directory, entry.name));
    } else if (entry.isFile()) {
      yield path.join(directory, entry.name);
    }
  }
}

export async function checkRepository(root) {
  const violations = [];
  for await (const file of walk(root)) {
    const relative = path.relative(root, file).replaceAll("\\", "/");
    const base = path.basename(file);
    const isCargoConfig = /(^|\/)\.cargo\/config(\.toml)?$/.test(relative);
    if (base === "Cargo.toml" || isCargoConfig) {
      violations.push(...checkCargoToml(relative, await readFile(file, "utf8")));
    } else if (relative.startsWith("scripts/") && SCRIPT_EXTENSIONS.has(path.extname(file))) {
      if (relative === "scripts/perf/check-build-config.mjs" || relative.endsWith(".test.mjs")) continue;
      violations.push(...checkScript(relative, await readFile(file, "utf8")));
    }
  }
  return violations.sort();
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
  const violations = await checkRepository(root);
  if (violations.length > 0) {
    console.error(["Build configuration guard failed:", ...violations.map((v) => `  - ${v}`)].join("\n"));
    process.exitCode = 1;
  } else {
    console.log("Build configuration guard passed (incremental dev builds, no local wrapper, no cargo clean).");
  }
}
