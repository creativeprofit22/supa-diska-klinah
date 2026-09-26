import { NavLink, Outlet } from "react-router-dom";
import { useStrings } from "../../shared/i18n/I18nProvider";
import { protectionStrings } from "./strings";
import "./protection.css";

const sections = [
  ["", "overview"],
  ["scan", "scan"],
  ["processes", "processes"],
  ["quarantine", "quarantine"],
  ["rules", "rules"],
  ["breach", "breach"],
] as const;

/** Local-first protection. Every network feature is off until turned on in Overview. */
export function ProtectionLayout() {
  const t = useStrings(protectionStrings).layout;
  return <section aria-labelledby="protection-title">
    <header className="page-header">
      <div>
        <p className="eyebrow">{t.eyebrow}</p>
        <h1 id="protection-title">{t.title}</h1>
        <p>{t.intro}</p>
      </div>
    </header>
    <nav aria-label={t.navLabel} className="protection-tabs">
      {sections.map(([path, key]) => <NavLink key={key} to={path === "" ? "/protection" : `/protection/${path}`} end={path === ""}>{t.sections[key]}</NavLink>)}
    </nav>
    <Outlet />
  </section>;
}
