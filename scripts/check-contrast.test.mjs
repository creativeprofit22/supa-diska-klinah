import assert from "node:assert/strict";
import test from "node:test";
import { checkFeatureStyles, checkStyles, contrast, parseHex } from "./check-contrast.mjs";

const root = `:root { background: #000000; --surface: #111111; --surface-strong: #222222;
  --text: #ffffff; --muted: #bbbbbb; --accent: #62c8ff; --danger: #ffb4ad; }`;
const motion = `@media (prefers-reduced-motion: reduce) { * { transition-duration: 0.01ms !important; animation-duration: 0.01ms !important; } }`;

test("computes WCAG contrast", () => {
  assert.equal(contrast(parseHex("#000"), parseHex("#fff")).toFixed(0), "21");
  assert.equal(contrast(parseHex("#777777"), parseHex("#ffffff")) < 4.5, true);
});

test("passes a compliant stylesheet", () => {
  assert.deepEqual(checkStyles(`${root} .b { color: #07131a; background: var(--accent); transition: color 1s; } ${motion}`), []);
});

test("flags low-contrast tokens and rule pairs", () => {
  const low = root.replace("--muted: #bbbbbb", "--muted: #444444");
  assert.match(checkStyles(`${low} ${motion}`).join("\n"), /--muted on --page/);
  assert.match(checkStyles(`${root} .x { color: #777777; background: #888888; } ${motion}`).join("\n"), /\.x:/);
});

test("flags missing reduced-motion coverage and px font sizes", () => {
  assert.match(checkStyles(`${root} .a { transition: color 1s; }`).join("\n"), /transitions/);
  assert.match(checkStyles(`${root} .a { font-size: 12px; } ${motion}`).join("\n"), /rem/);
});

test("feature stylesheets must use rem and tokens", () => {
  assert.deepEqual(checkFeatureStyles(".a { font-size: 0.875rem; color: var(--muted); border-color: CanvasText; }"), []);
  assert.equal(checkFeatureStyles(".a { font-size: 13px; }").length, 1);
  assert.equal(checkFeatureStyles(".a { color: #777; }").length, 1);
  assert.equal(checkFeatureStyles(".a { background: #fff; }").length, 1);
});
