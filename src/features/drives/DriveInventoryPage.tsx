import { useEffect } from "react";
import { useFormat, useStrings } from "../../shared/i18n/I18nProvider";
import { driveInventoryError } from "./api";
import { drivesStrings } from "./strings";
import { useDriveInventory } from "./useDriveInventory";

export function DriveInventoryPage() {
  const t = useStrings(drivesStrings);
  const fmt = useFormat();
  const { state, refresh } = useDriveInventory();
  const loading = state.kind === "loading";
  useEffect(() => { document.title = t.documentTitle; }, [t]);

  return (
    <section className="drive-inventory" aria-labelledby="drives-heading">
      <header className="page-header cleanup-page-header">
        <div>
          <p className="kicker">{t.kicker}</p>
          <h1 id="drives-heading">{t.heading}</h1>
          <p>{t.intro}</p>
        </div>
        <button type="button" disabled={loading} onClick={refresh}>
          {state.kind === "error" ? t.tryAgain : t.refresh}
        </button>
      </header>

      <p className="cleanup-diagnostics">{t.diagnostics}</p>
      <div role="status" className="status-message" aria-atomic="true">
        {loading && t.reading}
        {state.kind === "ready" && (state.inventory.partial
          ? t.incomplete(state.inventory.drives.length, fmt.number(state.inventory.drives.length))
          : t.found(state.inventory.drives.length, fmt.number(state.inventory.drives.length)))}
      </div>

      {state.kind === "error" && (
        <div className="cleanup-state-panel error-state" role="alert">
          <h2>{t.readError}</h2>
          <p>{driveInventoryError(state.reason, t.errors)}</p>
        </div>
      )}

      {state.kind === "ready" && (
        <>
          {state.inventory.partial && (
            <div className="cleanup-state-panel error-state">
              <h2>{t.partialHeading}</h2>
              <p>{t.partialBody}</p>
              {state.inventory.warnings.length > 0 && (
                <ul>
                  {state.inventory.warnings.map((warning, index) => (
                    <li key={index}>
                      {warning.code === "drive_unavailable"
                        ? t.driveUnavailable(warning.drive)
                        : t.inventoryPartial}
                    </li>
                  ))}
                </ul>
              )}
            </div>
          )}
          {state.inventory.drives.length === 0 && !state.inventory.partial && (
            <div className="cleanup-state-panel">
              <h2>{t.emptyHeading}</h2>
              <p>{t.emptyBody}</p>
            </div>
          )}
          {state.inventory.drives.length > 0 && (
            <ul className="drive-list" aria-label={t.listLabel}>
              {state.inventory.drives.map((drive) => (
                <li className="status-panel" key={drive.driveId}>
                  <div className="readiness-heading">
                    <h2>{drive.label.trim() || t.unlabelled} ({drive.displayMount})</h2>
                    {drive.system === true && <strong>{t.systemDrive}</strong>}
                    {drive.system === null && <span>{t.systemUnknown}</span>}
                  </div>
                  <dl className="status-list">
                    <div><dt>{t.fileSystem}</dt><dd>{drive.filesystem || t.notReported}</dd></div>
                    <div><dt>{t.totalCapacity}</dt><dd>{fmt.bytes(drive.totalBytes)}</dd></div>
                    <div><dt>{t.usedSpace}</dt><dd>{fmt.bytes(drive.usedBytes)}</dd></div>
                    <div><dt>{t.availableSpace}</dt><dd>{fmt.bytes(drive.freeBytes)}</dd></div>
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
