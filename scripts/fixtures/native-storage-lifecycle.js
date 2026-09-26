// Smoke-owned native WebView only. The immutable Tauri bridge is not replaced.
// Real registry reads; no folder authorization, vendor preparation or mutation.
(async probeCancellation => {
  const evidence = { cycles: 0, pages: 0, largestPage: 0, releasedSnapshots: 0,
    cancellationAttempts: 0, cancellationAcknowledgements: 0,
    uiCycles: 0, largestRenderedPage: 0, uiNextPages: 0, nextPages: 0 };
  const allowed = new Set(['start_program_inventory', 'storage_scan_status',
    'storage_scan_page', 'cancel_storage_scan', 'release_storage_scan']);
  const assert = (condition, message) => { if (!condition) throw Error(message); };
  const invoke = (command, input) => {
    assert(allowed.has(command), 'Unexpected command in read-only native smoke');
    return window.__TAURI_INTERNALS__.invoke(command, new TextEncoder().encode(JSON.stringify(input)));
  };
  const wait = async (predicate, label) => {
    for (let attempt = 0; attempt < 400; attempt++) {
      if (await predicate()) return;
      await new Promise(resolve => setTimeout(resolve, 25));
    }
    throw Error(`Native lifecycle timeout: ${label}`);
  };
  const button = text => [...document.querySelectorAll('button')].find(b => b.textContent.trim() === text);
  const query = { nameContains: '', largestFirst: false };
  // Real rendered controls, not a replay. Record counts only, never program names.
  for (let cycle = 0; cycle < 4; cycle++) {
    location.hash = '#/uninstaller';
    await wait(() => button('Refresh inventory'), 'inventory mount');
    button('Refresh inventory').click();
    await wait(() => document.querySelector('[aria-label="Installed program results"]') && document.querySelector('[aria-label="Scan result pages"][aria-busy="false"]'), 'rendered inventory');
    const count = document.querySelectorAll('[aria-label="Installed program results"] > li').length;
    assert(count <= 100, 'Rendered inventory exceeds page cap');
    evidence.largestRenderedPage = Math.max(evidence.largestRenderedPage, count);
    const next = button('Next page');
    if (next && !next.disabled) {
      const first = document.querySelector('[aria-label="Installed program results"] > li')?.textContent;
      next.click();
      await wait(() => document.querySelector('[aria-label="Scan result pages"][aria-busy="false"]') && document.querySelector('[aria-label="Installed program results"] > li')?.textContent !== first, 'rendered next page');
      const nextCount = document.querySelectorAll('[aria-label="Installed program results"] > li').length;
      assert(nextCount <= 100, 'Rendered paging appended instead of replacing');
      evidence.largestRenderedPage = Math.max(evidence.largestRenderedPage, nextCount);
      evidence.uiNextPages++;
    }
    location.hash = '#/disk-analyzer';
    await wait(() => document.querySelector('#analyzer-title'), 'inventory unmount');
    evidence.uiCycles++;
  }
  // Exercise snapshot authority separately through the real command boundary.
  // Page size one guarantees cursor use on any host with two inventory records.
  for (let cycle = 0; cycle < 4; cycle++) {
    const snapshotId = await invoke('start_program_inventory', query);
    const input = { module: 'uninstaller', snapshotId };
    try {
      await wait(async () => {
        const status = await invoke('storage_scan_status', input);
        assert(!['failed', 'cancelled'].includes(status.phase), 'Inventory did not complete');
        return status.phase === 'complete';
      }, 'native inventory completion');
      let page = await invoke('storage_scan_page', { ...input, collection: 'programs', pageSize: 1 });
      assert(page.snapshotId === snapshotId && page.records.length <= 1, 'Invalid native first page');
      evidence.pages++;
      evidence.largestPage = Math.max(evidence.largestPage, page.records.length);
      if (page.nextCursor) {
        page = await invoke('storage_scan_page', { ...input, collection: 'programs', pageSize: 1, cursor: page.nextCursor });
        assert(page.snapshotId === snapshotId && page.records.length <= 1, 'Invalid native next page');
        evidence.pages++;
        evidence.nextPages++;
      }
    } finally {
      await invoke('release_storage_scan', input);
    }
    let rejected = false;
    try { await invoke('storage_scan_page', { ...input, collection: 'programs', pageSize: 1 }); }
    catch (error) { rejected = error?.code === 'snapshot_unavailable'; }
    assert(rejected, 'Released snapshot retained native page authority');
    evidence.releasedSnapshots++;
    evidence.cycles++;
  }
  const snapshotId = await invoke('start_program_inventory', query);
  const input = { module: 'uninstaller', snapshotId };
  await probeCancellation(invoke, wait, input, evidence);
  return evidence;
})
