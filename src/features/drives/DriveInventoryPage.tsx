import { useEffect } from "react";
import { formatBytes } from "../cleanup/format";
import { useDriveInventory } from "./useDriveInventory";

export function DriveInventoryPage() {
  const { state, refresh } = useDriveInventory();
  const loading = state.kind === "loading";
  useEffect(() => { document.title = "Drives | Supa Diska Klinah"; }, []);

  return (
    <section className="drive-inventory" aria-labelledby="drives-heading">
      <header className="page-header cleanup-page-header">
        <div>
          <p className="kicker">Read-only inventory</p>
          <h1 id="drives-heading">Fixed drives</h1>
          <p>Review local drive capacity. No files are scanned, changed, or removed.</p>
        </div>
        <button type="button" disabled={loading} onClick={refresh}>
          {state.kind === "error" ? "Try again" : "Refresh drives"}
        </button>
      </header>

      <p className="cleanup-diagnostics">
        Capacity is reported by Windows for your account, including any quota limits.
        Removable, network, and optical drives are not included.
      </p>
      <div role="status" className="status-message" aria-atomic="true">
        {loading && "Reading fixed drives from Windows…"}
        {state.kind === "ready" && (state.inventory.partial
          ? `Incomplete inventory: ${state.inventory.drives.length} fixed drives available.`
          : `${state.inventory.drives.length} fixed ${state.inventory.drives.length === 1 ? "drive" : "drives"} found.`)}
      </div>

      {state.kind === "error" && (
        <div className="cleanup-state-panel error-state" role="alert">
          <h2>Could not read drive information</h2>
          <p>{state.message}</p>
        </div>
      )}

      {state.kind === "ready" && (
        <>
          {state.inventory.partial && (
            <div className="cleanup-state-panel error-state">
              <h2>Some drive information is unavailable</h2>
              <p>Missing information is not zero capacity. Refresh to try again.</p>
              {state.inventory.warnings.length > 0 && (
                <ul>
                  {state.inventory.warnings.map((warning, index) => (
                    <li key={index}>
                      {warning.code === "drive_unavailable"
                        ? `${warning.drive ?? "A fixed drive"}: Windows could not read this drive.`
                        : "Windows returned only part of the drive inventory."}
                    </li>
                  ))}
                </ul>
              )}
            </div>
          )}
          {state.inventory.drives.length === 0 && !state.inventory.partial && (
            <div className="cleanup-state-panel">
              <h2>No fixed drives found</h2>
              <p>Windows reported no eligible local fixed drives. Refresh to check again.</p>
            </div>
          )}
          {state.inventory.drives.length > 0 && (
            <ul className="drive-list" aria-label="Fixed drives">
              {state.inventory.drives.map((drive, index) => (
                <li className="status-panel" key={drive.driveId}>
                  <div className="readiness-heading">
                    <h2>{drive.label.trim() || `Unlabelled drive ${index + 1}`}</h2>
                    {drive.system === true && <strong>System drive</strong>}
                    {drive.system === null && <span>System classification unavailable</span>}
                  </div>
                  <dl className="status-list">
                    <div><dt>File system</dt><dd>{drive.filesystem || "Not reported"}</dd></div>
                    <div><dt>Total capacity</dt><dd>{formatBytes(drive.totalBytes)}</dd></div>
                    <div><dt>Used space</dt><dd>{formatBytes(drive.usedBytes)}</dd></div>
                    <div><dt>Available space</dt><dd>{formatBytes(drive.freeBytes)}</dd></div>
                  </dl>
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </section>
  );
}
