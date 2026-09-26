import { useCallback, useEffect, useMemo, useState } from "react";
import { useStrings } from "../../shared/i18n/I18nProvider";
import { systemChangeError } from "../../shared/system-change/api";
import { useSystemChangeLabels } from "../../shared/system-change/labels";
import { SystemChangeJournal } from "../../shared/system-change/SystemChangeJournal";
import { SystemChangeReview } from "../../shared/system-change/SystemChangeReview";
import type { SystemChange } from "../../shared/system-change/types";
import { getPrivacyReport } from "./api";
import { privacyStrings, type PrivacyStrings } from "./strings";
import type { PrivacyReport, PrivacySettingReport, PrivacyTaskReport, SettingCategory } from "./types";

const categories: SettingCategory[] = ["privacy", "performance"];

interface Option { key: string; label: string; change: SystemChange }

function settingOption(setting: PrivacySettingReport, t: PrivacyStrings): Option | null {
  if (!setting.supported || setting.recommended === null) return null;
  const kind = setting.hive === "user" ? "setUserSetting" : "setMachineSetting";
  if (setting.applied) {
    return { key: `setting:${setting.id}`, label: t.restoreDefaultFor(setting.label), change: { kind, settingId: setting.id, value: null } };
  }
  return { key: `setting:${setting.id}`, label: t.applyRecommendedFor(setting.label), change: { kind, settingId: setting.id, value: setting.recommended } };
}

function taskOption(task: PrivacyTaskReport, t: PrivacyStrings): Option | null {
  if (!task.present || task.enabled === null || task.enabled === task.recommended) return null;
  return { key: `task:${task.id}`, label: t.applyRecommendedFor(task.label), change: { kind: "setSystemTaskEnabled", catalogId: task.id, enabled: task.recommended } };
}

function valueText(value: number | null, t: PrivacyStrings): string {
  return value === null ? t.windowsDefault : String(value);
}

export function PrivacyPage() {
  const sc = useSystemChangeLabels();
  const t = useStrings(privacyStrings);
  const errors = sc.t.errors;
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
      setError(systemChangeError(reason, errors));
    } finally {
      setLoading(false);
    }
  }, [errors]);

  useEffect(() => { void refresh(); }, [refresh]);

  const options = useMemo(() => {
    const map = new Map<string, Option>();
    for (const setting of report?.settings ?? []) { const option = settingOption(setting, t); if (option) map.set(option.key, option); }
    for (const task of report?.tasks ?? []) { const option = taskOption(task, t); if (option) map.set(option.key, option); }
    return map;
  }, [report, t]);

  const changes = useMemo(() => selected.flatMap((key) => { const option = options.get(key); return option ? [option.change] : []; }), [options, selected]);

  const describe = useCallback((change: SystemChange): string => {
    switch (change.kind) {
      case "setUserSetting":
      case "setMachineSetting": {
        const label = report?.settings.find((setting) => setting.id === change.settingId)?.label ?? change.settingId;
        return change.value === null ? t.describeRestore(label) : t.describeApply(label);
      }
      case "setSystemTaskEnabled": {
        const label = report?.tasks.find((task) => task.id === change.catalogId)?.label ?? change.catalogId;
        return t.describeTask(change.enabled, label);
      }
      default:
        return change.kind;
    }
  }, [report, t]);

  const onFinished = useCallback(() => { setJournalKey((n) => n + 1); void refresh(); }, [refresh]);
  const toggle = (key: string) => setSelected((current) => current.includes(key) ? current.filter((value) => value !== key) : [...current, key]);

  const renderOption = (option: Option | null) => option && <label>
    <input type="checkbox" checked={selected.includes(option.key)} onChange={() => toggle(option.key)} />
    <span>{option.label}</span>
  </label>;

  return <section className="privacy-page" aria-labelledby="privacy-title">
    <header className="page-header"><div>
      <p className="eyebrow">{t.eyebrow}</p>
      <h1 id="privacy-title">{t.title}</h1>
      <p>{t.intro}</p>
    </div></header>
    <button type="button" disabled={loading} onClick={() => void refresh()}>{t.refresh}</button>
    {loading && <p role="status">{t.loading}</p>}
    {error && <p role="alert">{error}</p>}
    {report && <>
      {categories.map((category) => {
        const settings = report.settings.filter((setting) => setting.category === category);
        if (settings.length === 0) return null;
        return <section key={category} aria-labelledby={`privacy-category-${category}`}>
          <h2 id={`privacy-category-${category}`}>{t.category[category]}</h2>
          <ul>{settings.map((setting) => <li key={setting.id}>
            <strong>{setting.label}</strong>
            <p>{setting.description}</p>
            {setting.supported
              ? <p>{setting.hive === "user" ? t.yourAccount : t.allUsers}{t.current(valueText(setting.current, t))}{t.recommended(valueText(setting.recommended, t))}{setting.applied ? t.recommendedApplied : ""}</p>
              : <p>{t.unavailable(setting.unsupportedReason ? sc.unsupported[setting.unsupportedReason] : sc.t.notSupportedOnDevice)}</p>}
            {renderOption(settingOption(setting, t))}
          </li>)}</ul>
        </section>;
      })}
      <section aria-labelledby="privacy-tasks">
        <h2 id="privacy-tasks">{t.tasksTitle}</h2>
        {report.tasks.length === 0 && <p>{t.noTasks}</p>}
        <ul>{report.tasks.map((task) => <li key={task.id}>
          <strong>{task.label}</strong>
          <p className="privacy-path">{task.path}</p>
          <p>{!task.present || task.enabled === null
            ? t.taskNotPresent
            : t.taskState(task.enabled, task.recommended)}</p>
          {renderOption(taskOption(task, t))}
        </li>)}</ul>
      </section>
      {report.relatedServices.length > 0 && <p>{t.relatedServices(report.relatedServices.join(", "))}</p>}
    </>}
    <SystemChangeReview changes={changes} describe={describe} onFinished={onFinished} />
    <SystemChangeJournal module="privacy" describe={describe} refreshKey={journalKey} onFinished={onFinished} />
  </section>;
}
