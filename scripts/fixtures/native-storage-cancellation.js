// Shared by the native WebView smoke and deterministic Node tests.
(async (invoke, wait, input, evidence) => {
  let acknowledged = false;
  try {
    evidence.cancellationAttempts++;
    try {
      await invoke('cancel_storage_scan', input);
      acknowledged = true;
      evidence.cancellationAcknowledgements++;
    } catch (error) {
      if (error?.code !== 'snapshot_unavailable') throw error;
    }
    await wait(async () => {
      const status = await invoke('storage_scan_status', input);
      if (status?.module !== input.module || status?.snapshotId !== input.snapshotId) {
        throw Error('Cancellation status identity mismatch');
      }
      // A refused request proves nothing: only finalization followed by completion
      // can reconcile the expected snapshot_unavailable race.
      if (status.phase === 'complete' || (acknowledged && status.phase === 'cancelled')) {
        evidence.cancelOutcome = status.phase;
        return true;
      }
      const pending = acknowledged
        ? ['queued', 'walking', 'grouping', 'partialHash', 'fullHash', 'finalizing']
        : ['finalizing'];
      if (!pending.includes(status.phase)) throw Error('Invalid cancellation terminal state');
      return false;
    }, 'cancelled worker retirement');
  } finally {
    await invoke('release_storage_scan', input);
    evidence.releasedSnapshots++;
  }
})
