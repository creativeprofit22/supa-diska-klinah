import { useEffect } from "react";
import { useFoundationStatus } from "./model/useFoundationStatus";

export function DashboardPage() {
  const { status, error, loading, retry } = useFoundationStatus();

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
    </section>
  );
}
