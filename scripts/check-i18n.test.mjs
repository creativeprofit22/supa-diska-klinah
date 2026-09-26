import assert from "node:assert/strict";
import test from "node:test";
import { findHardCodedText } from "./check-i18n.mjs";

const texts = (source) => findHardCodedText(source).map((finding) => finding.text);

test("flags JSX text and text attributes", () => {
  assert.deepEqual(texts(`const A = () => <p>Scan now</p>;`), ["Scan now"]);
  assert.deepEqual(texts(`const A = () => <button aria-label="Close">{t.x}</button>;`), [`aria-label="Close"`]);
  assert.deepEqual(texts(`const A = () => <input placeholder={"Search"} />;`), [`placeholder="Search"`]);
  assert.deepEqual(texts(`const A = () => <img alt='Logo' title={\`Drive \${n}\`} />;`), [`alt="Logo"`, `title="Drive "`]);
  assert.deepEqual(texts(`const A = () => <p>{"Loading…"}</p>;`), ["Loading…"]);
  assert.deepEqual(texts(`const A = () => <p>{busy ? "Working" : t.idle}</p>;`), ["Working"]);
  assert.deepEqual(texts(`const A = () => <p>Análisis</p>;`), ["Análisis"]);
});

test("allows catalog reads, punctuation and non-text attributes", () => {
  assert.deepEqual(texts(`const A = () => <p className="row" role="status" data-testid="x">{t.title} · {fmt.bytes(n)} (…)</p>;`), []);
  assert.deepEqual(texts(`const A = () => <section aria-label={t.region} aria-labelledby="h1-id"><h2 id="h1-id">{t.h}</h2></section>;`), []);
  assert.deepEqual(texts(`const A = () => <p>{" · "}{count}%</p>;`), []);
});

test("reports line numbers", () => {
  const [finding] = findHardCodedText("const A = () => (\n  <div>\n    <p>Hello</p>\n  </div>\n);");
  assert.equal(finding?.line, 3);
});
