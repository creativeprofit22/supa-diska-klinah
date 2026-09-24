import { NavLink, Outlet } from "react-router-dom";
import "./protection.css";

const sections = [
  ["", "Overview"],
  ["scan", "Scan"],
  ["processes", "Processes"],
  ["quarantine", "Quarantine"],
  ["rules", "Rules"],
  ["breach", "Password check"],
] as const;

/** Local-first protection. Every network feature is off until turned on in Overview. */
export function ProtectionLayout() {
  return <section aria-labelledby="protection-title">
    <header className="page-header">
      <div>
        <p className="eyebrow">Diagnostics</p>
        <h1 id="protection-title">Protection</h1>
        <p>Checks files and processes on this PC using signed rules and local heuristics. It works offline and complements your antivirus; it does not replace it.</p>
      </div>
    </header>
    <nav aria-label="Protection sections" className="protection-tabs">
      {sections.map(([path, label]) => <NavLink key={label} to={path === "" ? "/protection" : `/protection/${path}`} end={path === ""}>{label}</NavLink>)}
    </nav>
    <Outlet />
  </section>;
}
