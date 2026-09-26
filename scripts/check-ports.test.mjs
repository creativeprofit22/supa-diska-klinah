import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { resolveConfig } from "vite";

test("resolved Vite development and preview bind only the assigned strict ports", async () => {
  const config = await resolveConfig({}, "serve");
  assert.equal(config.server.host, "127.0.0.1");
  assert.equal(config.server.port, 1520);
  assert.equal(config.server.strictPort, true);
  assert.equal(config.preview.host, "127.0.0.1");
  assert.equal(config.preview.port, 1521);
  assert.equal(config.preview.strictPort, true);
});

test("Tauri development origin and HMR match development, not preview or GG Coder", async () => {
  const config = JSON.parse(await readFile("src-tauri/tauri.conf.json", "utf8"));
  assert.equal(config.build.devUrl, "http://127.0.0.1:1520");
  assert.match(config.app.security.devCsp, /ws:\/\/127\.0\.0\.1:1520(?:;|\s)/);
  assert.doesNotMatch(config.app.security.devCsp, /:(?:1420|1421|1521)(?:;|\s)/);
  assert.ok(!config.app.security.csp.includes("127.0.0.1"));
  const harness = await readFile("scripts/smoke-storage-frontend.mjs", "utf8");
  assert.match(harness, /STORAGE_UI_PORT \?\? 1521/);
  assert.match(harness, /port === 1520 \|\| port === 1521/);
});
