import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { declarationDigest, localDeclarations, upstreamDeclarations } from './storage-catalog-contract.mjs';

const read = path => JSON.parse(readFileSync(new URL(path, import.meta.url), 'utf8'));
const pinned = read('./fixtures/pinned-browser-layouts.json');
const actual = read('../src-tauri/crates/cleanup-core/rules/browser-caches.json');
const revision = 'db09e051d0615121e659db187e3799438acbc9e6';
const cleaner = read('./fixtures/pinned-cleaner-declarations.json');

test('cleaner reference covers all seven pinned catalogs', () => {
  assert.equal(cleaner.revision, revision);
  assert.deepEqual(cleaner.catalogs.map(row => row.local).sort(), ['apps-caches.json', 'database-maintenance.json', 'gaming-caches.json', 'gpu-caches.json', 'misc-caches.json', 'steam-caches.json', 'system-caches.json']);
});
for (const reference of cleaner.catalogs) {
  test(`compiled ${reference.local} matches every pinned declaration in its comparison scope`, () => {
    assert.match(reference.local, /^[a-z-]+\.json$/);
    assert.match(reference.sourceSha256, /^[a-f0-9]{64}$/);
    const catalog = read(`../src-tauri/crates/cleanup-core/rules/${reference.local}`);
    assert.equal(catalog.source, reference.source);
    assert.deepEqual(declarationDigest(localDeclarations(catalog)), { count: reference.count, sha256: reference.sha256 });
  });
}

test('reference normalization preserves scope, exact file rules, age and recursive exclusions', () => {
  const source = { apps: [{ paths: ['${PROGRAMFILES_X86}/Tool'], minAgeDays: 2,
    fileMatch: { childDirSuffix: '-updater', names: ['installer.exe'], skipIfChildExists: ['pending'], minAgeDays: 14 },
    recursiveMatch: { targets: ['Cache'], excludedAncestors: ['Cookies'], maxDepth: 8 } }] };
  const declarations = upstreamDeclarations(source);
  assert.deepEqual(declarations, [{ path: 'programFilesX86/Tool/*-updater', age: 1_209_600,
    recursive: source.apps[0].recursiveMatch, files: ['installer.exe'], blockers: ['pending'] }]);
  const changed = structuredClone(declarations);
  changed[0].recursive.excludedAncestors = [];
  assert.notDeepEqual(declarationDigest(changed), declarationDigest(declarations));
  assert.throws(() => upstreamDeclarations({ apps: [{ paths: ['${UNREVIEWED_ROOT}/Cache'] }] }), /Unknown reference variable/);
});

test('unsupported database and Steam maintenance cannot silently become selectable', () => {
  for (const file of ['database-maintenance', 'steam-caches', 'misc-caches']) {
    const catalog = read(`../src-tauri/crates/cleanup-core/rules/${file}.json`);
    delete catalog.targets[0].unsupported;
    assert.throws(() => localDeclarations(catalog), /Unsupported maintenance gained authority/);
  }
});

test('compiled browser catalog matches all pinned Windows layout reference fields', () => {
  assert.equal(pinned.revision, revision);
  assert.equal(actual.revision, revision);
  assert.equal(actual.source, pinned.source);
  assert.deepEqual(actual.profile, pinned.chromiumCacheDirs.profile);
  assert.deepEqual(actual.shared, pinned.chromiumCacheDirs.shared);
  assert.deepEqual(actual.chromium.map(([key, scope, path]) => {
    assert.ok(['local', 'roaming'].includes(scope));
    return { key, base: `${scope === 'local' ? '${LOCALAPPDATA}' : '${APPDATA}'}/${path}` };
  }), pinned.chromium);
  assert.deepEqual(actual.firefox.map(([key, base, cache]) => ({
    key, base: '${APPDATA}/' + base, cache: '${LOCALAPPDATA}/' + cache,
  })), [{ key: 'firefox', ...pinned.firefox }, ...pinned.firefoxForks]);
  assert.equal(pinned.safari, null);
  assert.ok(actual.unsupported.some(reason => reason.includes('Safari')));
});

test('pinned browser translation retains explicit personal-data safety adaptations', () => {
  for (const name of ['Cookies', 'Login Data', 'Bookmarks', 'History', 'Sessions', 'Local Storage',
    'IndexedDB', 'Extensions', 'places.sqlite', 'cookies.sqlite', 'key4.db', 'logins.json']) {
    assert.ok(actual.exclusions.includes(name), `Missing private-data exclusion: ${name}`);
  }
  assert.ok(actual.minimumAgeSeconds > 0);
  assert.match(actual.serviceWorkerDisclosure, /Explicit opt-in/);
});
