import { useCallback, useEffect, useMemo, useState } from "react";
import { systemChangeError } from "../../shared/system-change/api";
import { unsupportedLabel } from "../../shared/system-change/labels";
import { SystemChangeJournal } from "../../shared/system-change/SystemChangeJournal";
import { SystemChangeReview } from "../../shared/system-change/SystemChangeReview";
import type { SystemChange } from "../../shared/system-change/types";
import { getPrivacyReport } from "./api";
import type { PrivacyReport, PrivacySettingReport, PrivacyTaskReport, SettingCategory } from "./types";

const categoryTitle: Record<SettingCategory, string> = { privacy: "Privacy", performance: "Performance" };
const categories: SettingCategory[] = ["privacy", "performance"];

interface Option { key: string; label: string; change: SystemChange }

function settingOption(setting: PrivacySettingReport): Option | null {
  if (!setting.supported || setting.recommended === null) return null;
  const kind = setting.hive === "user" ? "setUserSetting" : "setMachineSetting";
  if (setting.applied) {
    return { key: `setting:${setting.id}`, label: `Restore Windows default for ${setting.label}`, change: { kind, settingId: setting.id, value: null } };
  }
  return { key: `setting:${setting.id}`, label: `Apply recommended for ${setting.label}`, change: { kind, settingId: setting.id, value: setting.recommended } };
}

function taskOption(task: PrivacyTaskReport): Option | null {
  if (!task.present || task.enabled === null || task.enabled === task.recommended) return null;
  return { key: `task:${task.id}`, label: `Apply recommended for ${task.label}`, change: { kind: "setSystemTaskEnabled", catalogId: task.id, enabled: task.recommended } };
}

function valueText(value: number | null): string {
  return value === null ? "Windows default" : String(value);
}

export function PrivacyPage() {
  const [report, setReport] = useState<PrivacyReport | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string[]>([]);
  const [journalKey, setJournalKey] = useState(0);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setReport(await getPrivacyReport());
      setSelected([]);
    } catch (reason) {
      setError(systemChangeError(reason));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { void refresh(); }, [refresh]);

  const options = useMemo(() => {
    const map = new Map<string, Option>();
    for (const setting of report?.settings ?? []) { const option = settingOption(setting); if (option) map.set(option.key, option); }
    for (const task of report?.tasks ?? []) { const option = taskOption(task); if (option) map.set(option.key, option); }
    return map;
  }, [report]);

  const changes = useMemo(() => selected.flatMap((key) => { const option = options.get(key); return option ? [option.change] : []; }), [options, selected]);

  const describe = useCallback((change: SystemChange): string => {
    switch (change.kind) {
      case "setUserSetting":
      case "setMachineSetting": {
        const label = report?.settings.find((setting) => setting.id === change.settingId)?.label ?? change.settingId;
        return change.value === null ? `Restore Windows default: ${label}` : `Apply recommended setting: ${label}`;
      }
      case "setSystemTaskEnabled": {
        const label = report?.tasks.find((task) => task.id === change.catalogId)?.label ?? change.catalogId;
        return `${change.enabled ? "Enable" : "Disable"} scheduled task: ${label}`;
      }
      default:
        return change.kind;
    }
  }, [report]);

  const onFinished = useCallback(() => { setJournalKey((n) => n + 1); void refresh(); }, [refresh]);
  const toggle = (key: string) => setSelected((current) => current.includes(key) ? current.filter((value) => value !== key) : [...current, key]);

  const renderOption = (option: Option | null) => option && <label>
    <input type="checkbox" checked={selected.includes(option.key)} onChange={() => toggle(option.key)} />
    <span>{option.label}</span>
  </label>;

  return <section className="privacy-page" aria-labelledby="privacy-title">
    <header className="page-header"><div>
      <p className="eyebrow">System</p>
      <h1 id="privacy-title">Privacy</h1>
      <p>Review Windows privacy and telemetry settings; nothing changes until you select an item and confirm it in Windows.</p>
    </div></header>
    <button type="button" disabled={loading} onClick={() => void refresh()}>Refresh</button>
    {loading && <p role="status">Reading privacy settings…</p>}
    {error && <p role="alert">{error}</p>}
    {report && <>
      {categories.map((category) => {
        const settings = report.settings.filter((setting) => setting.category === category);
        if (settings.length === 0) return null;
        return <section key={category} aria-labelledby={`privacy-category-${category}`}>
          <h2 id={`privacy-category-${category}`}>{categoryTitle[category]}</h2>
          <ul>{settings.map((setting) => <li key={setting.id}>
            <strong>{setting.label}</strong>
            <p>{setting.description}</p>
            {setting.supported
              ? <p>{setting.hive === "user" ? "Your account" : "All users (needs administrator)"} · Current: {valueText(setting.current)} · Recommended: {valueText(setting.recommended)}{setting.applied ? " · Recommended value applied" : ""}</p>
              : <p>Unavailable: {setting.unsupportedReason ? unsupportedLabel[setting.unsupportedReason] : "Not supported on this device"}</p>}
            {renderOption(settingOption(setting))}
          </li>)}</ul>
        </section>;
      })}
      <section aria-labelledby="privacy-tasks">
        <h2 id="privacy-tasks">Scheduled tasks</h2>
        {report.tasks.length === 0 && <p>No telemetry tasks are listed.</p>}
        <ul>{report.tasks.map((task) => <li key={task.id}>
          <strong>{task.label}</strong>
          <p className="privacy-path">{task.path}</p>
          <p>{!task.present || task.enabled === null
            ? "Not present on this device"
            : `${task.enabled ? "Enabled" : "Disabled"} · Recommended: ${task.recommended ? "enabled" : "disabled"}`}</p>
          {renderOption(taskOption(task))}
        </li>)}</ul>
      </section>
      {report.relatedServices.length > 0 && <p>
        Related telemetry services ({report.relatedServices.join(", ")}) are managed on the Services page.
      </p>}
    </>}
    <SystemChangeReview changes={changes} describe={describe} onFinished={onFinished} />
    <SystemChangeJournal module="privacy" describe={describe} refreshKey={journalKey} onFinished={onFinished} />
  </section>;
}
