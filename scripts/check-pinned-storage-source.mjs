// Optional bounded, read-only source verification. CI's offline parity check
// uses the resulting reviewed fixtures; this command never updates them.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { declarationDigest, localDeclarations, upstreamDeclarations } from './storage-catalog-contract.mjs';

const read = path => JSON.parse(readFileSync(new URL(path, import.meta.url), 'utf8'));
const reference = read('./fixtures/pinned-cleaner-declarations.json');
const revision = 'db09e051d0615121e659db187e3799438acbc9e6';
assert.equal(reference.revision, revision);
assert.equal(reference.catalogs.length, 7);
async function fetchPinned(path) {
  assert.match(path, /^rules\/win32\/[a-z-]+\.json$/);
  const response = await fetch(`https://raw.githubusercontent.com/AdventDevInc/kudu/${revision}/${path}`, {
    signal: AbortSignal.timeout(20000), redirect: 'error',
  });
  assert.ok(response.ok, `Pinned source returned HTTP ${response.status}`);
  const chunks = [];
  let bytes = 0;
  for await (const chunk of response.body) {
    bytes += chunk.byteLength;
    assert.ok(bytes <= 262144, 'Pinned source exceeded 256 KiB limit');
    chunks.push(chunk);
  }
  const raw = Buffer.concat(chunks);
  return { source: JSON.parse(raw.toString('utf8')), sha256: createHash('sha256').update(raw).digest('hex') };
}
let declarations = 0;
for (const catalog of reference.catalogs) {
  assert.match(catalog.local, /^[a-z-]+\.json$/);
  const upstream = await fetchPinned(catalog.source);
  assert.equal(upstream.sha256, catalog.sourceSha256, `Source bytes changed: ${catalog.source}`);
  const expected = { count: catalog.count, sha256: catalog.sha256 };
  assert.deepEqual(declarationDigest(upstreamDeclarations(upstream.source)), expected);
  assert.deepEqual(declarationDigest(localDeclarations(read(`../src-tauri/crates/cleanup-core/rules/${catalog.local}`))), expected);
  declarations += catalog.count;
}
const browser = read('./fixtures/pinned-browser-layouts.json');
assert.equal(browser.revision, revision);
const { source } = await fetchPinned(browser.source);
assert.deepEqual(Object.fromEntries(Object.entries(source.chromiumCacheDirs).map(([scope, rows]) => [scope, rows.map(row => row.dir)])), browser.chromiumCacheDirs);
assert.deepEqual(source.chromium.map(({ key, base }) => ({ key, base })), browser.chromium);
assert.deepEqual({ base: source.firefox.base, cache: source.firefox.cache }, browser.firefox);
assert.deepEqual(source.firefoxForks.map(({ key, base, cache }) => ({ key, base, cache })), browser.firefoxForks);
assert.equal(source.safari, browser.safari);
console.log(`Pinned source verified: ${declarations} cleaner declarations across seven catalogs and all Windows browser layout fields. No upstream code executed or fixtures changed.`);
