import { useCallback, useEffect, useMemo, useState } from "react";
import { useFormat, useStrings } from "../../shared/i18n/I18nProvider";
import { SystemChangeJournal } from "../../shared/system-change/SystemChangeJournal";
import { SystemChangeReview } from "../../shared/system-change/SystemChangeReview";
import type { ScheduleCadence, SystemChange, Weekday } from "../../shared/system-change/types";
import { listScanSchedules, listScheduledScanSummaries } from "./api";
import { schedulerStrings, type SchedulerStrings } from "./strings";
import type { ScanSchedule, ScheduledScanSummary } from "./types";

const weekdays: Weekday[] = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];

const pad = (value: number) => String(value).padStart(2, "0");
function cadenceLabel(cadence: ScheduleCadence, t: SchedulerStrings): string {
  const time = `${pad(cadence.hour)}:${pad(cadence.minute)}`;
  return cadence.kind === "daily" ? t.daily(time) : t.weekly(t.weekday[cadence.day], time);
}

function errorText(reason: unknown, fallback: string): string {
  const message = (reason as { message?: unknown } | null)?.message;
  return typeof message === "string" && message.length > 0 && message.length <= 300 ? message : fallback;
}

function parseTime(value: string): { hour: number; minute: number } | null {
  const match = /^(\d{2}):(\d{2})$/.exec(value);
  if (!match) return null;
  const hour = Number(match[1]);
  const minute = Number(match[2]);
  return hour <= 23 && minute <= 59 ? { hour, minute } : null;
}

type PendingUpsert = Extract<SystemChange, { kind: "upsertScanSchedule" }>;

export function SchedulerPage() {
  const t = useStrings(schedulerStrings);
  const fmt = useFormat();
  const [schedules, setSchedules] = useState<ScanSchedule[] | null>(null);
  const [summaries, setSummaries] = useState<ScheduledScanSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [frequency, setFrequency] = useState<"daily" | "weekly">("daily");
  const [day, setDay] = useState<Weekday>("monday");
  const [time, setTime] = useState("03:00");
  const [formError, setFormError] = useState<string | null>(null);
  const [pending, setPending] = useState<PendingUpsert[]>([]);
  const [removals, setRemovals] = useState<string[]>([]);
  const [journalKey, setJournalKey] = useState(0);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [list, recent] = await Promise.all([listScanSchedules(), listScheduledScanSummaries()]);
      setSchedules(list);
      setSummaries(recent);
      setPending([]);
      setRemovals([]);
      setError(null);
    } catch (reason) {
      setError(errorText(reason, t.loadError));
    } finally {
      setLoading(false);
    }
  }, [t]);
  useEffect(() => { void refresh(); }, [refresh]);

  const addSchedule = () => {
    const parsed = parseTime(time);
    if (!parsed) { setFormError(t.timeFormat); return; }
    setFormError(null);
    const cadence: ScheduleCadence = frequency === "daily"
      ? { kind: "daily", hour: parsed.hour, minute: parsed.minute }
      : { kind: "weekly", day, hour: parsed.hour, minute: parsed.minute };
    setPending((current) => [...current, { kind: "upsertScanSchedule", scheduleId: crypto.randomUUID().toLowerCase(), cadence }]);
  };

  const changes = useMemo<SystemChange[]>(() => [
    ...pending,
    ...removals.map((scheduleId): SystemChange => ({ kind: "removeScanSchedule", scheduleId })),
  ], [pending, removals]);

  const describe = useCallback((change: SystemChange) => {
    if (change.kind === "upsertScanSchedule") return t.describeAdd(cadenceLabel(change.cadence, t));
    if (change.kind === "removeScanSchedule") {
      const cadence = schedules?.find((schedule) => schedule.id === change.scheduleId)?.cadence;
      return t.describeRemove(cadence ? cadenceLabel(cadence, t) : change.scheduleId);
    }
    return change.kind;
  }, [schedules, t]);

  const toggleRemoval = (id: string) => setRemovals((current) => current.includes(id) ? current.filter((value) => value !== id) : [...current, id]);
  const onFinished = useCallback(() => { setJournalKey((value) => value + 1); void refresh(); }, [refresh]);

  return <section className="scheduler-page" aria-labelledby="scheduler-title">
    <header className="page-header"><div>
      <p className="eyebrow">{t.eyebrow}</p>
      <h1 id="scheduler-title">{t.title}</h1>
      <p>{t.intro}</p>
    </div></header>
    <button type="button" disabled={loading} onClick={() => void refresh()}>{t.refresh}</button>
    {loading && <p role="status">{t.loading}</p>}
    {error && <p role="alert">{error}</p>}

    <form className="scheduler-form" aria-label={t.newScheduleLabel} onSubmit={(event) => { event.preventDefault(); addSchedule(); }}>
      <label>{t.frequency}
        <select value={frequency} onChange={(event) => setFrequency(event.target.value === "weekly" ? "weekly" : "daily")}>
          <option value="daily">{t.dailyOption}</option>
          <option value="weekly">{t.weeklyOption}</option>
        </select>
      </label>
      {frequency === "weekly" && <label>{t.weekdayLabel}
        <select value={day} onChange={(event) => setDay(event.target.value as Weekday)}>
          {weekdays.map((value) => <option key={value} value={value}>{t.weekday[value]}</option>)}
        </select>
      </label>}
      <label>{t.time}
        <input type="time" value={time} required onChange={(event) => setTime(event.target.value)} />
      </label>
      <button type="submit">{t.addSchedule}</button>
      {formError && <p role="alert">{formError}</p>}
    </form>

    {pending.length > 0 && <>
      <h2>{t.pendingTitle}</h2>
      <ul className="scheduler-pending">{pending.map((change) => <li key={change.scheduleId}>
        {cadenceLabel(change.cadence, t)}{" "}
        <button type="button" onClick={() => setPending((current) => current.filter((entry) => entry.scheduleId !== change.scheduleId))}
          aria-label={t.discardLabel(cadenceLabel(change.cadence, t))}>{t.discard}</button>
      </li>)}</ul>
    </>}

    <h2>{t.existingTitle}</h2>
    {schedules && schedules.length === 0 && <p>{t.noSchedules}</p>}
    {schedules && schedules.length > 0 && <ul className="scheduler-list">{schedules.map((schedule) => {
      const removable = schedule.cadence !== null;
      return <li key={schedule.id}>
        <strong>{schedule.cadence ? cadenceLabel(schedule.cadence, t) : t.unrecognized}</strong>
        <p>{schedule.enabled ? t.enabled : t.disabled}{t.lastRun(schedule.lastRun ?? t.never)}{t.nextRun(schedule.nextRun ?? t.notScheduled)}</p>
        {schedule.orphaned && <p>{t.orphaned(schedule.orphanReason ? t.orphan[schedule.orphanReason] : t.orphanedFallback)}</p>}
        {!removable && <p>{t.cannotRemove}</p>}
        <label>
          <input type="checkbox" disabled={!removable} checked={removals.includes(schedule.id)} onChange={() => toggleRemoval(schedule.id)} />
          {t.removeSchedule}
        </label>
      </li>;
    })}</ul>}

    <SystemChangeReview changes={changes} describe={describe} onFinished={onFinished} />

    <h2>{t.recentTitle}</h2>
    {summaries.length === 0
      ? <p>{t.noRecent}</p>
      : <ul className="scheduler-summaries">{summaries.map((summary) => <li key={`${summary.scheduleId}:${summary.finishedAt}`}>
        {fmt.dateTime(summary.finishedAt) ?? t.unknownTime}: {summary.succeeded
          ? t.summary(fmt.bytes(summary.reclaimableBytes), fmt.number(summary.itemCount), fmt.number(summary.diagnosticCount))
          : t.didNotFinish}
      </li>)}</ul>}

    <SystemChangeJournal module="scheduler" describe={describe} refreshKey={journalKey} onFinished={onFinished} />
  </section>;
}
