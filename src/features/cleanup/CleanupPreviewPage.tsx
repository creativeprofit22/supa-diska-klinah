import { useEffect } from "react";
import type { PreviewRecord } from "./api/previewCleanup";
import { useCleanupPreview } from "./model/useCleanupPreview";

const byteFormatter = new Intl.NumberFormat();

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${byteFormatter.format(bytes)} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = -1;
  do {
    value /= 1024;
    unit += 1;
  } while (value >= 1024 && unit < units.length - 1);
  return `${value.toLocaleString(undefined, { maximumFractionDigits: 1 })} ${units[unit]}`;
}

function formatModified(seconds: number): string | null {
  const date = new Date(seconds * 1000);
  return Number.isNaN(date.getTime()) ? null : date.toLocaleString();
}

function ruleLabel(ruleId: string): string {
  // simplification: Catalog-provided localized labels replace this sentence-case fallback later.
  const label = ruleId.replaceAll("-", " ");
  return label.charAt(0).toUpperCase() + label.slice(1);
}

function groupRecords(records: PreviewRecord[]): [string, PreviewRecord[]][] {
  const groups = new Map<string, PreviewRecord[]>();
  for (const record of records) {
    const group = groups.get(record.ruleId) ?? [];
    group.push(record);
    groups.set(record.ruleId, group);
  }
  return [...groups.entries()].sort(([left], [right]) => left.localeCompare(right));
}

export function CleanupPreviewPage() {
  const { result, error, loading, retry } = useCleanupPreview();
  const records = result?.records ?? [];
  const groups = groupRecords(records);
  const totalBytes = records.reduce((total, record) => total + record.bytes, 0);

  useEffect(() => {
    document.title = "Cleanup preview | Supa Diska Klinah";
  }, []);

  return (
    <section aria-labelledby="cleanup-heading">
      <header className="page-header cleanup-page-header">
        <div>
          <p className="kicker">Temporary files</p>
          <h1 id="cleanup-heading">Cleanup preview</h1>
          <p>
            Checks cache and tmp directories in your Windows temporary folder.
            Nothing is removed.
          </p>
        </div>
        <button type="button" disabled={loading} onClick={retry}>
          {loading ? "Scanning…" : error ? "Try again" : "Scan again"}
        </button>
      </header>

      {loading && (
        <div className="cleanup-state-panel" role="status">
          <h2>Scanning temporary caches</h2>
          <p>Checking the fixed preview location. Nothing will be changed.</p>
        </div>
      )}

      {error && (
        <div className="cleanup-state-panel error-state" role="alert">
          <h2>Could not complete the preview</h2>
          <p>Windows could not check the temporary folder. Try the scan again.</p>
        </div>
      )}

      {result && records.length === 0 && (
        <div className="cleanup-state-panel">
          <h2>Nothing found</h2>
          <p>No cache or tmp directories were found in your Windows temporary folder.</p>
          {result.diagnostics.length > 0 && (
            <p className="cleanup-diagnostics">
              {result.diagnostics.length} skipped {result.diagnostics.length === 1 ? "location" : "locations"}
            </p>
          )}
        </div>
      )}

      {result && records.length > 0 && (
        <div className="cleanup-results">
          <div className="cleanup-summary" role="status">
            <strong>
              {records.length} {records.length === 1 ? "item" : "items"}
            </strong>
            <span>{formatBytes(totalBytes)} total</span>
          </div>

          {result.diagnostics.length > 0 && (
            <p className="cleanup-diagnostics">
              {result.diagnostics.length} skipped {result.diagnostics.length === 1 ? "location" : "locations"}
            </p>
          )}

          {groups.map(([ruleId, group], index) => {
            const headingId = `cleanup-group-${index}`;
            return (
              <section
                className="cleanup-group"
                aria-labelledby={headingId}
                key={ruleId}
              >
                <h2 id={headingId}>{ruleLabel(ruleId)}</h2>
                <ul className="cleanup-records">
                  {group.map((record) => {
                    const modified =
                      record.modifiedUnixSeconds == null
                        ? null
                        : formatModified(record.modifiedUnixSeconds);
                    return (
                      <li key={record.id}>
                        <strong className="cleanup-path">{record.displayPath}</strong>
                        <div className="cleanup-record-meta">
                          <span>
                            {record.kind === "directory" ? "Directory" : "File"} ·{" "}
                            {formatBytes(record.bytes)}
                          </span>
                          {modified && (
                            <time dateTime={new Date(record.modifiedUnixSeconds! * 1000).toISOString()}>
                              Modified {modified}
                            </time>
                          )}
                        </div>
                      </li>
                    );
                  })}
                </ul>
              </section>
            );
          })}
        </div>
      )}
    </section>
  );
}
