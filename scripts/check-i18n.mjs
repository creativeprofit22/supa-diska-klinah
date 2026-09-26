// Fails when UI components hard-code user-visible text instead of reading it from
// a typed catalog (`strings.ts`). Scans .tsx files under src/features and src/shared,
// excluding tests, `testing/` fixtures and catalogs. Flags:
//   - JSX text containing a letter;
//   - string literals (direct or in `{...}`) in aria-label, aria-description, title,
//     placeholder, alt and label attributes.
import { readdirSync, readFileSync } from "node:fs";
import { relative, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";
import ts from "typescript";

const TEXT_ATTRIBUTES = new Set(["aria-label", "aria-description", "aria-roledescription", "title", "placeholder", "alt", "label"]);
const LETTER = /\p{L}/u;

function literalText(expression) {
  if (!expression) return null;
  if (ts.isStringLiteral(expression) || ts.isNoSubstitutionTemplateLiteral(expression)) return expression.text;
  if (ts.isTemplateExpression(expression)) {
    const parts = [expression.head.text, ...expression.templateSpans.map((span) => span.literal.text)];
    return parts.join(" ");
  }
  if (ts.isConditionalExpression(expression)) {
    return [literalText(expression.whenTrue), literalText(expression.whenFalse)].filter(Boolean).join(" ") || null;
  }
  if (ts.isParenthesizedExpression(expression)) return literalText(expression.expression);
  return null;
}

/** Returns `{ line, text }` findings for one component source file. */
export function findHardCodedText(source, fileName = "component.tsx") {
  const file = ts.createSourceFile(fileName, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const findings = [];
  const report = (node, text) => {
    const { line } = file.getLineAndCharacterOfPosition(node.getStart(file));
    findings.push({ line: line + 1, text: text.trim().replace(/\s+/g, " ").slice(0, 80) });
  };
  const visit = (node) => {
    if (ts.isJsxText(node) && LETTER.test(node.text)) {
      report(node, node.text);
    } else if (ts.isJsxExpression(node) && ts.isJsxElement(node.parent) || ts.isJsxExpression(node) && ts.isJsxFragment(node.parent)) {
      const text = literalText(node.expression);
      if (text && LETTER.test(text)) report(node, text);
    } else if (ts.isJsxAttribute(node) && TEXT_ATTRIBUTES.has(node.name.getText(file))) {
      const initializer = node.initializer;
      const text = initializer && ts.isStringLiteral(initializer)
        ? initializer.text
        : initializer && ts.isJsxExpression(initializer) ? literalText(initializer.expression) : null;
      if (text && LETTER.test(text)) report(node, `${node.name.getText(file)}="${text}"`);
    }
    ts.forEachChild(node, visit);
  };
  visit(file);
  return findings;
}

function componentFiles(directory) {
  const entries = readdirSync(directory, { withFileTypes: true });
  return entries.flatMap((entry) => {
    const path = resolve(directory, entry.name);
    // `testing/` holds test-only fixtures, not shipped UI.
    if (entry.isDirectory()) return entry.name === "testing" ? [] : componentFiles(path);
    if (!entry.name.endsWith(".tsx") || /\.test\.tsx$/.test(entry.name) || entry.name === "strings.tsx") return [];
    return [path];
  }).sort();
}

export function checkRepository(root) {
  const failures = [];
  for (const directory of ["src/features", "src/shared"]) {
    for (const path of componentFiles(resolve(root, directory))) {
      for (const finding of findHardCodedText(readFileSync(path, "utf8"), path)) {
        failures.push(`${relative(root, path).split(sep).join("/")}:${finding.line}: ${finding.text}`);
      }
    }
  }
  return failures;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  const failures = checkRepository(resolve(import.meta.dirname, ".."));
  if (failures.length > 0) {
    console.error("Hard-coded UI text found. Move it into the feature's strings.ts catalog (en + es419):");
    for (const failure of failures) console.error(`  ${failure}`);
    process.exit(1);
  }
  console.log("No hard-coded UI text in feature or shared components.");
}
