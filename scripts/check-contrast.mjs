// Checks the colour tokens and rule-level colour pairs in src/styles.css meet
// WCAG 2.2 AA contrast for normal text (4.5:1), and that every transition or
// animation is neutralised by the prefers-reduced-motion block.
import { readdirSync, readFileSync } from "node:fs";
import { relative, resolve } from "node:path";
import { pathToFileURL } from "node:url";

export const MIN_TEXT_CONTRAST = 4.5;

// Every text colour token must be readable on every surface it is drawn on.
const TEXT_TOKENS = ["--text", "--muted", "--accent", "--danger"];
const SURFACE_TOKENS = ["--page", "--surface", "--surface-strong"];

function channel(value) {
  const c = value / 255;
  return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
}

export function parseHex(hex) {
  const match = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(hex.trim());
  if (!match) return null;
  const digits = match[1].length === 3 ? [...match[1]].map((d) => d + d).join("") : match[1];
  return [0, 2, 4].map((i) => Number.parseInt(digits.slice(i, i + 2), 16));
}

export function contrast(foreground, background) {
  const lum = ([r, g, b]) => 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
  const [a, b] = [lum(foreground), lum(background)].sort((x, y) => y - x);
  return (a + 0.05) / (b + 0.05);
}

function stripComments(css) {
  return css.replace(/\/\*[\s\S]*?\*\//g, "");
}

/** Top-level `selector { declarations }` blocks, including those nested in @media. */
function blocks(css) {
  const result = [];
  const pattern = /([^{}]+)\{([^{}]*)\}/g;
  for (const match of stripComments(css).matchAll(pattern)) {
    result.push({ selector: match[1].trim(), body: match[2] });
  }
  return result;
}

function declarations(body) {
  const map = new Map();
  for (const part of body.split(";")) {
    const index = part.indexOf(":");
    if (index > 0) map.set(part.slice(0, index).trim(), part.slice(index + 1).trim());
  }
  return map;
}

export function checkStyles(css) {
  const failures = [];
  const all = blocks(css);
  const root = all.find((block) => block.selector === ":root");
  if (!root) return ["styles.css has no :root block"];
  const tokens = declarations(root.body);
  const rootColors = new Map([...tokens].filter(([name]) => name.startsWith("--")));
  const page = tokens.get("background");
  if (page) rootColors.set("--page", page);
  const resolveColor = (value) => {
    if (!value) return null;
    const token = /^var\((--[\w-]+)\)$/.exec(value);
    return parseHex(token ? rootColors.get(token[1]) ?? "" : value);
  };

  for (const text of TEXT_TOKENS) {
    for (const surface of SURFACE_TOKENS) {
      const fg = resolveColor(`var(${text})`);
      const bg = resolveColor(`var(${surface})`);
      if (!fg || !bg) {
        failures.push(`missing colour token ${fg ? surface : text}`);
        continue;
      }
      const ratio = contrast(fg, bg);
      if (ratio < MIN_TEXT_CONTRAST) failures.push(`${text} on ${surface}: ${ratio.toFixed(2)}:1`);
    }
  }

  for (const block of all) {
    if (block.selector === ":root") continue;
    const decl = declarations(block.body);
    const fg = resolveColor(decl.get("color"));
    const bg = resolveColor(decl.get("background") ?? decl.get("background-color"));
    if (fg && bg) {
      const ratio = contrast(fg, bg);
      if (ratio < MIN_TEXT_CONTRAST) failures.push(`${block.selector}: ${ratio.toFixed(2)}:1`);
    }
  }

  const clean = stripComments(css);
  const reduced = /@media\s*\(prefers-reduced-motion:\s*reduce\)\s*\{([\s\S]*?\})\s*\}/.exec(clean)?.[1] ?? "";
  if (/\btransition\b/.test(clean) && !/transition-duration:\s*0\.01ms\s*!important/.test(reduced)) {
    failures.push("transitions are not disabled under prefers-reduced-motion");
  }
  if (/\banimation\b/.test(clean.replace(reduced, "")) && !/animation-duration:\s*0\.01ms\s*!important/.test(reduced)) {
    failures.push("animations are not disabled under prefers-reduced-motion");
  }
  if (/font-size:\s*[\d.]+px/.test(clean)) failures.push("font sizes must use rem so Windows text scaling applies");
  return failures;
}

/** Rules for feature stylesheets, which inherit tokens and motion rules from styles.css. */
export function checkFeatureStyles(css) {
  const failures = [];
  const clean = stripComments(css);
  if (/font-size:\s*[\d.]+px/.test(clean)) failures.push("font sizes must use rem so Windows text scaling applies");
  if (/(?:^|[^-])color:\s*#/m.test(clean) || /background(?:-color)?:\s*#/.test(clean)) {
    failures.push("colours must use the checked tokens from styles.css");
  }
  return failures;
}

function featureStylesheets(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = resolve(directory, entry.name);
    if (entry.isDirectory()) return featureStylesheets(path);
    return entry.name.endsWith(".css") ? [path] : [];
  });
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  const src = resolve(import.meta.dirname, "../src");
  const main = resolve(src, "styles.css");
  const failures = checkStyles(readFileSync(main, "utf8"));
  for (const path of featureStylesheets(src).filter((path) => path !== main).sort()) {
    for (const failure of checkFeatureStyles(readFileSync(path, "utf8"))) {
      failures.push(`${relative(src, path)}: ${failure}`);
    }
  }
  if (failures.length > 0) {
    console.error("Accessibility style check failed:");
    for (const failure of failures) console.error(`  ${failure}`);
    process.exit(1);
  }
  console.log("Colour contrast, text sizing and reduced-motion coverage verified.");
}
