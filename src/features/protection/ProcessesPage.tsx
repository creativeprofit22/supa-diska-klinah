import { useCallback, useEffect, useMemo, useState } from "react";
import { useFormat, useStrings } from "../../shared/i18n/I18nProvider";
import { listProcesses } from "./api";
import { EvidenceBadge, errorMessage, evidenceDetail, signerLabel } from "./labels";
import { protectionStrings } from "./strings";
import type { ProcessInventory } from "./types";

export function ProcessesPage() {
  const strings = useStrings(protectionStrings);
  const t = strings.processes;
  const fmt = useFormat();
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
      setError(errorMessage(reason, t.loadFailed));
    } finally {
      setLoading(false);
    }
  }, [t]);
  useEffect(() => { void refresh(); }, [refresh]);

  const rows = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    const all = inventory?.processes ?? [];
    const matching = needle ? all.filter((p) => p.name.toLowerCase().includes(needle) || p.imagePath?.toLowerCase().includes(needle)) : all;
    return [...matching].sort((a, b) => Number(b.findings.some((f) => f.kind === "heuristic")) - Number(a.findings.some((f) => f.kind === "heuristic")) || a.name.localeCompare(b.name));
  }, [inventory, filter]);

  return <section aria-labelledby="processes-title">
    <h2 id="processes-title">{t.title}</h2>
    <p>{t.intro}</p>
    <div className="protection-actions">
      <button type="button" disabled={loading} onClick={() => void refresh()}>{t.refresh}</button>
      <label>{t.filter} <input type="search" value={filter} onChange={(event) => setFilter(event.target.value)} /></label>
    </div>
    {loading && <p role="status">{t.loading}</p>}
    {error && <p role="alert">{error}</p>}
    {inventory && <>
      <p>{t.count(fmt.number(inventory.processes.length))}{inventory.imageUnavailableCount > 0 ? t.imageUnavailable(fmt.number(inventory.imageUnavailableCount)) : ""}{inventory.truncated ? t.truncated : ""}</p>
      <table className="protection-table">
        <thead><tr><th scope="col">{t.columns.name}</th><th scope="col">{t.columns.pid}</th><th scope="col">{t.columns.parent}</th><th scope="col">{t.columns.signature}</th><th scope="col">{t.columns.findings}</th></tr></thead>
        <tbody>{rows.map((process) => <tr key={process.pid}>
          <th scope="row">{process.name}{process.imagePath && <><br /><code>{process.imagePath}</code></>}</th>
          <td>{process.pid}</td>
          <td>{process.parentPid}</td>
          <td>{signerLabel(process.signer, strings)}</td>
          <td>{process.findings.length === 0 ? t.none : <ul>{process.findings.map((finding, index) => <li key={index}><EvidenceBadge evidence={finding} /> {evidenceDetail(finding, strings)}</li>)}</ul>}</td>
        </tr>)}</tbody>
      </table>
    </>}
  </section>;
}
