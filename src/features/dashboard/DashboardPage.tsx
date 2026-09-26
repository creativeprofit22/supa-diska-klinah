import { useEffect } from "react";
import { Link } from "react-router-dom";
import { useStrings } from "../../shared/i18n/I18nProvider";
import { useFoundationStatus } from "./model/useFoundationStatus";
import { dashboardStrings } from "./strings";

export function DashboardPage() {
  const t = useStrings(dashboardStrings);
  const { status, error, loading, retry } = useFoundationStatus();

  useEffect(() => {
    document.title = t.documentTitle;
  }, [t]);

  return (
    <section aria-labelledby="dashboard-heading">
      <header className="page-header">
        <p className="kicker">{t.kicker}</p>
        <h1 id="dashboard-heading">{t.heading}</h1>
        <p>{t.intro}</p>
      </header>

      <div className="status-panel">
        {loading && (
          <p className="status-message" role="status">
            {t.checking}
          </p>
        )}

        {error && (
          <div className="error-state" role="alert">
            <h2>{t.unreachable}</h2>
            <p>{error}</p>
            <button type="button" onClick={retry}>
              {t.tryAgain}
            </button>
          </div>
        )}

        {status && (
          <>
            <div className="readiness-heading">
              <h2>{t.adapterStatus}</h2>
              <strong>{status.adapterReady ? t.ready : t.unavailable}</strong>
            </div>
            <dl className="status-list">
              <div>
                <dt>{t.platform}</dt>
                <dd>{status.platform}</dd>
              </div>
              <div>
                <dt>{t.architecture}</dt>
                <dd>{status.architecture}</dd>
              </div>
              <div>
                <dt>{t.nativeAdapter}</dt>
                <dd>{status.adapterReady ? t.connected : t.notConnected}</dd>
              </div>
            </dl>
          </>
        )}
      </div>

      <section className="restore-point-panel" aria-labelledby="restore-point-heading">
        <h2 id="restore-point-heading">{t.restorePointHeading}</h2>
        <p>{t.restorePointBody}</p>
        <Link to="/restore-points">{t.restorePointLink}</Link>
      </section>
    </section>
  );
}
