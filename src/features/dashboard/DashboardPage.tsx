import { type FormEvent, useEffect, useState } from "react";
import { useCreateSystemRestorePoint } from "./model/useCreateSystemRestorePoint";
import { useFoundationStatus } from "./model/useFoundationStatus";

const DEFAULT_RESTORE_POINT_DESCRIPTION = "Supa Diska Klinah safety restore point";

export function DashboardPage() {
  const { status, error, loading, retry } = useFoundationStatus();
  const restorePoint = useCreateSystemRestorePoint();
  const [confirmingRestorePoint, setConfirmingRestorePoint] = useState(false);
  const [description, setDescription] = useState(DEFAULT_RESTORE_POINT_DESCRIPTION);

  function createRestorePoint(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setConfirmingRestorePoint(false);
    void restorePoint.create({ description });
  }

  useEffect(() => {
    document.title = "Dashboard | Supa Diska Klinah";
  }, []);

  return (
    <section aria-labelledby="dashboard-heading">
      <header className="page-header">
        <p className="kicker">Foundation</p>
        <h1 id="dashboard-heading">System readiness</h1>
        <p>Confirm the native Windows adapter before cleanup features arrive.</p>
      </header>

      <div className="status-panel">
        {loading && (
          <p className="status-message" role="status">
            Checking the native adapter…
          </p>
        )}

        {error && (
          <div className="error-state" role="alert">
            <h2>Could not reach the native adapter</h2>
            <p>{error}</p>
            <button type="button" onClick={retry}>
              Try again
            </button>
          </div>
        )}

        {status && (
          <>
            <div className="readiness-heading">
              <h2>Adapter status</h2>
              <strong>{status.adapterReady ? "Ready" : "Unavailable"}</strong>
            </div>
            <dl className="status-list">
              <div>
                <dt>Platform</dt>
                <dd>{status.platform}</dd>
              </div>
              <div>
                <dt>Architecture</dt>
                <dd>{status.architecture}</dd>
              </div>
              <div>
                <dt>Native adapter</dt>
                <dd>{status.adapterReady ? "Connected" : "Not connected"}</dd>
              </div>
            </dl>
          </>
        )}
      </div>

      <div className="restore-point-panel" aria-labelledby="restore-point-heading">
        <h2 id="restore-point-heading">System restore point</h2>
        <p>
          Save Windows system settings before future cleanup actions. This is not
          a file backup.
        </p>

        {!confirmingRestorePoint && (
          <button
            type="button"
            disabled={restorePoint.loading}
            onClick={() => setConfirmingRestorePoint(true)}
          >
            {restorePoint.loading ? "Creating restore point…" : "Create restore point"}
          </button>
        )}

        {confirmingRestorePoint && (
          <form className="restore-point-confirmation" onSubmit={createRestorePoint}>
            <label htmlFor="restore-point-description">Restore point description</label>
            <input
              id="restore-point-description"
              name="description"
              type="text"
              required
              maxLength={128}
              aria-describedby="restore-point-confirmation-help"
              value={description}
              onChange={(event) => setDescription(event.target.value)}
            />
            <p id="restore-point-confirmation-help">
              Windows will ask for administrator approval. Canceling that prompt
              leaves this app open.
            </p>
            <div className="button-row">
              <button type="submit">Confirm and continue</button>
              <button
                className="secondary-button"
                type="button"
                onClick={() => setConfirmingRestorePoint(false)}
              >
                Cancel
              </button>
            </div>
          </form>
        )}

        {restorePoint.result && (
          <p className="success-state" role="status">
            Restore point created. Sequence number: {restorePoint.result.sequenceNumber}
          </p>
        )}

        {restorePoint.error && (
          <p className="error-message" role="alert">
            {restorePoint.error}
          </p>
        )}
      </div>
    </section>
  );
}
