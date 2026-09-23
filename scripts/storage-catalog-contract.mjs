// Test-only canonical declarations. Never resolves paths or executes upstream code.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';

const variables = { LOCALAPPDATA: 'local', APPDATA: 'roaming', HOME: 'profile',
  PROGRAMDATA: 'programData', WINDIR: 'windows', PROGRAMFILES: 'programFiles',
  PROGRAMFILES_X86: 'programFilesX86' };
function pathKey(path) {
  return path.replace(/^C:\/Windows\.old$/, 'systemDrive/Windows.old').replace('${HOME}/AppData/LocalLow', 'low').replace(/\$\{([^}]+)\}/g, (_, key) => {
    assert.ok(Object.hasOwn(variables, key), `Unknown reference variable: ${key}`);
    return variables[key];
  });
}
function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === 'object') return Object.fromEntries(Object.keys(value).sort().map(key => [key, canonical(value[key])]));
  return value;
}
export function declarationDigest(declarations) {
  const rows = declarations.map(row => JSON.stringify(canonical(row))).sort();
  return { count: rows.length, sha256: createHash('sha256').update(JSON.stringify(rows)).digest('hex') };
}
export function upstreamDeclarations(source) {
  if (source.type === 'system') return [
    ...source.cleanTargets.map(rule => ({ path: pathKey(rule.path), singleFile: false })),
    ...source.singleFileTargets.map(rule => ({ path: pathKey(rule.path), singleFile: true })),
  ];
  if (source.type === 'databases') return source.targets.flatMap(rule => {
    const files = Array.isArray(rule.dbFiles) ? rule.dbFiles : source.sharedDbFileSets[rule.dbFiles.slice(1)];
    return (rule.multiProfile ? rule.profilePattern ?? ['*'] : ['']).flatMap(profile =>
      files.map(file => ({ path: [pathKey(rule.basePath), profile, file].filter(Boolean).join('/') })));
  });
  if (source.type === 'steam') return [
    ...source.libraries.map(path => ({ library: pathKey(path) })),
    ...source.redistPatterns.map(pattern => ({ redistributable: pattern })),
  ];
  if (source.type === 'misc') {
    assert.equal(source.trashPath, null);
    return source.protectedEventLogs.map(name => ({ protectedEventLog: name }));
  }
  assert.ok(Array.isArray(source.apps));
  return source.apps.flatMap(rule => rule.paths.map(path => {
    let resolved = pathKey(path);
    if (rule.childSubdir) resolved += `/*/${rule.childSubdir}`;
    if (rule.fileMatch?.childDirSuffix) resolved += `/*${rule.fileMatch.childDirSuffix}`;
    return { path: resolved, age: rule.fileMatch?.minAgeDays !== undefined ? rule.fileMatch.minAgeDays * 86400 : rule.minAgeDays !== undefined ? rule.minAgeDays * 86400 : null,
      recursive: rule.recursiveMatch ?? null, files: rule.fileMatch?.names ?? null,
      blockers: rule.fileMatch?.skipIfChildExists ?? null };
  }));
}
export function localDeclarations(source) {
  assert.ok(Array.isArray(source.targets));
  if (['databases', 'steam', 'misc'].includes(source.id)) {
    for (const rule of source.targets) assert.ok(rule.unsupported?.length > 0, `Unsupported maintenance gained authority: ${rule.id}`);
  }
  if (source.id === 'system') return source.targets.flatMap(rule => rule.paths.map(path => ({ path, singleFile: rule.singleFile ?? false })));
  if (source.id === 'databases') return source.targets.flatMap(rule => rule.paths.flatMap(path =>
    (rule.suffixes ?? ['']).map(suffix => ({ path: suffix ? `${path}/${suffix}` : path }))));
  if (source.id === 'steam') return [
    ...source.targets.find(rule => rule.id === 'libraries').paths.map(path => ({ library: path.replace(/^unsupported\//, '') })),
    ...source.targets.find(rule => rule.id === 'redistributables').suffixes.map(pattern => ({ redistributable: pattern })),
  ];
  if (source.id === 'misc') {
    assert.deepEqual(source.targets.find(rule => rule.id === 'trash').paths, ['unsupported/native-recycle-bin']);
    return source.targets.find(rule => rule.id === 'protected-event-logs').suffixes.map(name => ({ protectedEventLog: name }));
  }
  return source.targets.flatMap(rule => rule.paths.flatMap(path => (rule.suffixes ?? ['']).map(suffix => ({
    path: suffix ? `${path}/${suffix}` : path, age: rule.minimumAgeSeconds ?? null,
    recursive: rule.recursiveMatch ?? null, files: rule.filePatterns ?? null,
    blockers: rule.blockers ?? null,
  }))));
}
