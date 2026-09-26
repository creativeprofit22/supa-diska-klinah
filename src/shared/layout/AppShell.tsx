import { NavLink, Outlet } from "react-router-dom";
import { useStrings } from "../i18n/I18nProvider";
import { layoutStrings } from "./strings";
import { UpdateRecoveryNotice } from "./UpdateRecoveryNotice";

export function AppShell() {
  const t = useStrings(layoutStrings);
  return (
    <div className="app-shell">
      <a className="skip-link" href="#main-content" onClick={event => {
        event.preventDefault();
        const main = document.getElementById("main-content");
        main?.focus();
        main?.scrollIntoView({ block: "start" });
      }}>
        {t.skipToContent}
      </a>
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark" aria-hidden="true">{t.brandMark}</span>
          <span>
            <strong>{t.productName}</strong>
            <small>{t.brandSubtitle}</small>
          </span>
        </div>
        <nav aria-label={t.primaryNavigation}>
          <NavLink to="/" end>
            {t.nav.dashboard}
          </NavLink>
          <NavLink to="/drives">{t.nav.drives}</NavLink>
          <NavLink to="/disk-analyzer">{t.nav.diskAnalyzer}</NavLink>
          <NavLink to="/large-files">{t.nav.largeFiles}</NavLink>
          <NavLink to="/duplicates">{t.nav.duplicates}</NavLink>
          <NavLink to="/empty-folders">{t.nav.emptyFolders}</NavLink>
          <NavLink to="/cleaner">{t.nav.cleaner}</NavLink>
          <NavLink to="/browser">{t.nav.browser}</NavLink>
          <NavLink to="/uninstaller">{t.nav.uninstaller}</NavLink>
          <NavLink to="/cleanup">{t.nav.cleanup}</NavLink>
          <NavLink to="/optimizer">{t.nav.optimizer}</NavLink>
          <NavLink to="/startup">{t.nav.startup}</NavLink>
          <NavLink to="/services">{t.nav.services}</NavLink>
          <NavLink to="/privacy">{t.nav.privacy}</NavLink>
          <NavLink to="/firewall">{t.nav.firewall}</NavLink>
          <NavLink to="/hosts">{t.nav.hosts}</NavLink>
          <NavLink to="/power">{t.nav.power}</NavLink>
          <NavLink to="/drivers">{t.nav.drivers}</NavLink>
          <NavLink to="/restore-points">{t.nav.restorePoints}</NavLink>
          <NavLink to="/windows-update">{t.nav.windowsUpdate}</NavLink>
          <NavLink to="/scheduled-scans">{t.nav.scheduledScans}</NavLink>
          <NavLink to="/protection">{t.nav.protection}</NavLink>
          <NavLink to="/settings">{t.nav.settings}</NavLink>
        </nav>
      </aside>
      <main id="main-content" tabIndex={-1}>
        <UpdateRecoveryNotice />
        <Outlet />
      </main>
    </div>
  );
}
