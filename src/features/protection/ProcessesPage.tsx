import { useCallback, useEffect, useMemo, useState } from "react";
import { listProcesses } from "./api";
import { EvidenceBadge, errorMessage, evidenceDetail, signerLabel } from "./labels";
import type { ProcessInventory } from "./types";

export function ProcessesPage() {
  const [inventory, setInventory] = useState<ProcessInventory | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState("");

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setInventory(await listProcesses());
      setError(null);
    } catch (reason) {
      setError(errorMessage(reason, "The process list could not be read."));
    } finally {
      setLoading(false);
    }
  }, []);
  useEffect(() => { void refresh(); }, [refresh]);

  const rows = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    const all = inventory?.processes ?? [];
    const matching = needle ? all.filter((p) => p.name.toLowerCase().includes(needle) || p.imagePath?.toLowerCase().includes(needle)) : all;
    return [...matching].sort((a, b) => Number(b.findings.some((f) => f.kind === "heuristic")) - Number(a.findings.some((f) => f.kind === "heuristic")) || a.name.localeCompare(b.name));
  }, [inventory, filter]);

  return <section aria-labelledby="processes-title">
    <h2 id="processes-title">Running processes</h2>
    <p>Read-only. This app never ends or changes processes; use Task Manager for that. Command lines are not read.</p>
    <div className="protection-actions">
      <button type="button" disabled={loading} onClick={() => void refresh()}>Refresh</button>
      <label>Filter <input type="search" value={filter} onChange={(event) => setFilter(event.target.value)} /></label>
    </div>
    {loading && <p role="status">Reading processes…</p>}
    {error && <p role="alert">{error}</p>}
    {inventory && <>
      <p>{inventory.processes.length} processes{inventory.imageUnavailableCount > 0 ? ` · ${inventory.imageUnavailableCount} could not be inspected (usually system or other users' processes)` : ""}{inventory.truncated ? " · list truncated" : ""}</p>
      <table className="protection-table">
        <thead><tr><th scope="col">Name</th><th scope="col">PID</th><th scope="col">Parent</th><th scope="col">Signature</th><th scope="col">Findings</th></tr></thead>
        <tbody>{rows.map((process) => <tr key={process.pid}>
          <th scope="row">{process.name}{process.imagePath && <><br /><code>{process.imagePath}</code></>}</th>
          <td>{process.pid}</td>
          <td>{process.parentPid}</td>
          <td>{signerLabel(process.signer)}</td>
          <td>{process.findings.length === 0 ? "None" : <ul>{process.findings.map((finding, index) => <li key={index}><EvidenceBadge evidence={finding} /> {evidenceDetail(finding)}</li>)}</ul>}</td>
        </tr>)}</tbody>
      </table>
    </>}
  </section>;
}
