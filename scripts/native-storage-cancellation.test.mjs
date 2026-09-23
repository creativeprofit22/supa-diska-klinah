import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { runInNewContext } from 'node:vm';

// Execute the same trusted local function expression injected into the WebView.
const probe = runInNewContext(readFileSync(new URL('./fixtures/native-storage-cancellation.js', import.meta.url), 'utf8'));
const input = { module: 'uninstaller', snapshotId: 'a'.repeat(32) };
const unavailable = { code: 'snapshot_unavailable' };
const ipcError = { code: 'storage_unavailable' };
const status = phase => ({ ...input, phase });

const cases = [
  { name: 'acknowledged active cancellation retires as cancelled', phases: ['walking', 'finalizing', 'cancelled'], outcome: 'cancelled' },
  { name: 'completion before cancel reconciles snapshot_unavailable', cancelError: unavailable, phases: ['complete'], outcome: 'complete' },
  { name: 'finalization before cancel reconciles snapshot_unavailable', cancelError: unavailable, phases: ['finalizing', 'complete'], outcome: 'complete' },
  { name: 'completion wins after acknowledgement', phases: ['complete'], outcome: 'complete' },
  { name: 'failed after acknowledgement is rejected', phases: ['failed'], failure: /terminal state/ },
  { name: 'failed after refusal is rejected', cancelError: unavailable, phases: ['failed'], failure: /terminal state/ },
  { name: 'unacknowledged cancelled is rejected', cancelError: unavailable, phases: ['cancelled'], failure: /terminal state/ },
  { name: 'unexplained active scan after refusal is rejected', cancelError: unavailable, phases: ['walking'], failure: /terminal state/ },
  { name: 'unexpected cancel IPC error propagates without status fallback', cancelError: ipcError, phases: [], failure: ipcError },
  { name: 'disappearance after refusal propagates', cancelError: unavailable, statusError: unavailable, failure: unavailable },
  { name: 'disappearance after acknowledgement propagates', statusError: unavailable, failure: unavailable },
  { name: 'unexpected status IPC error propagates', statusError: ipcError, failure: ipcError },
  { name: 'wrong module rejected', response: { ...status('complete'), module: 'browser' }, failure: /identity mismatch/ },
  { name: 'wrong snapshot rejected', cancelError: unavailable, response: { ...status('complete'), snapshotId: 'b'.repeat(32) }, failure: /identity mismatch/ },
  { name: 'missing status rejected', response: null, failure: /identity mismatch/ },
  { name: 'unknown phase rejected', phases: ['unknown'], failure: /terminal state/ },
  { name: 'nonterminal timeout rejects and releases', phases: ['finalizing', 'finalizing', 'finalizing'], failure: /test poll budget/ },
  { name: 'release error prevents success', phases: ['cancelled'], releaseError: ipcError, failure: ipcError },
];

for (const scenario of cases) {
  test(scenario.name, async () => {
    const evidence = { cancellationAttempts: 0, cancellationAcknowledgements: 0, releasedSnapshots: 0 };
    const calls = [];
    let polls = 0;
    const invoke = async (command, body) => {
      assert.equal(body, input, 'Every command uses the same module/snapshot input');
      calls.push(command);
      switch (command) {
        case 'cancel_storage_scan':
          assert.equal(evidence.cancellationAttempts, 1);
          if (scenario.cancelError) throw scenario.cancelError;
          return;
        case 'storage_scan_status':
          polls++;
          if (scenario.statusError) throw scenario.statusError;
          if ('response' in scenario) return scenario.response;
          assert.ok(polls <= scenario.phases.length, 'Unexpected status query');
          return status(scenario.phases[polls - 1]);
        case 'release_storage_scan':
          if (scenario.releaseError) throw scenario.releaseError;
          return;
        default: assert.fail(`Forbidden command: ${command}`);
      }
    };
    // Deterministic bounded polling: no clocks or sleeps.
    const wait = async predicate => {
      for (let attempt = 0; attempt < 3; attempt++) if (await predicate()) return;
      throw Error('test poll budget');
    };
    const result = probe(invoke, wait, input, evidence);
    if (scenario.failure instanceof RegExp) await assert.rejects(result, scenario.failure);
    else if (scenario.failure) await assert.rejects(result, error => error === scenario.failure);
    else await result;
    assert.equal(evidence.cancellationAttempts, 1);
    assert.equal(evidence.cancellationAcknowledgements, scenario.cancelError ? 0 : 1);
    assert.equal(evidence.cancelOutcome, scenario.outcome ?? (scenario.releaseError ? 'cancelled' : undefined));
    assert.equal(evidence.releasedSnapshots, scenario.releaseError ? 0 : 1);
    assert.equal(calls[0], 'cancel_storage_scan');
    assert.equal(calls.at(-1), 'release_storage_scan');
    assert.equal(calls.filter(command => command === 'release_storage_scan').length, 1);
    if (scenario.cancelError === ipcError) assert.equal(polls, 0);
  });
}
