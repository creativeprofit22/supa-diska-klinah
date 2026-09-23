import { NavLink, Outlet } from "react-router-dom";

export function AppShell() {
  return (
    <div className="app-shell">
      <a className="skip-link" href="#main-content" onClick={event => {
        event.preventDefault();
        const main = document.getElementById("main-content");
        main?.focus();
        main?.scrollIntoView({ block: "start" });
      }}>
        Skip to content
      </a>
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark" aria-hidden="true">S</span>
          <span>
            <strong>Supa Diska Klinah</strong>
            <small>Windows foundation</small>
          </span>
        </div>
        <nav aria-label="Primary navigation">
          <NavLink to="/" end>
            Dashboard
          </NavLink>
          <NavLink to="/drives">Drives</NavLink>
          <NavLink to="/disk-analyzer">Disk analyzer</NavLink>
          <NavLink to="/large-files">Large files</NavLink>
          <NavLink to="/duplicates">Duplicate files</NavLink>
          <NavLink to="/empty-folders">Empty folders</NavLink>
          <NavLink to="/cleaner">Rule cleaner</NavLink>
          <NavLink to="/browser">Browser caches</NavLink>
          <NavLink to="/uninstaller">Installed programs</NavLink>
          <NavLink to="/cleanup">Cleanup</NavLink>
          <NavLink to="/optimizer">Quick optimization</NavLink>
          <NavLink to="/startup">Startup apps</NavLink>
          <NavLink to="/services">Windows services</NavLink>
          <NavLink to="/privacy">Privacy</NavLink>
          <NavLink to="/firewall">Firewall</NavLink>
          <NavLink to="/hosts">Hosts file</NavLink>
          <NavLink to="/power">Power and hibernation</NavLink>
          <NavLink to="/drivers">Driver packages</NavLink>
          <NavLink to="/restore-points">Restore points</NavLink>
          <NavLink to="/windows-update">Windows Update</NavLink>
          <NavLink to="/scheduled-scans">Scheduled scans</NavLink>
          <NavLink to="/settings">Settings</NavLink>
        </nav>
      </aside>
      <main id="main-content" tabIndex={-1}>
        <Outlet />
      </main>
    </div>
  );
}
