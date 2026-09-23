import { useCallback, useEffect, useMemo, useState } from "react";
import { formatBytes } from "../../shared/format";
import { SystemChangeJournal } from "../../shared/system-change/SystemChangeJournal";
import { SystemChangeReview } from "../../shared/system-change/SystemChangeReview";
import type { ScheduleCadence, SystemChange, Weekday } from "../../shared/system-change/types";
import { listScanSchedules, listScheduledScanSummaries } from "./api";
import type { OrphanReason, ScanSchedule, ScheduledScanSummary } from "./types";

const weekdays: { value: Weekday; label: string }[] = [
  { value: "monday", label: "Monday" }, { value: "tuesday", label: "Tuesday" }, { value: "wednesday", label: "Wednesday" },
  { value: "thursday", label: "Thursday" }, { value: "friday", label: "Friday" }, { value: "saturday", label: "Saturday" },
  { value: "sunday", label: "Sunday" },
];
const weekdayLabel = (day: Weekday) => weekdays.find((entry) => entry.value === day)?.label ?? day;

const orphanLabel: Record<OrphanReason, string> = {
  foreignExecutable: "The task runs a different program than this installation.",
  malformedArguments: "The task's arguments were changed outside this app.",
  unrecognizedDefinition: "The task's triggers or actions were changed outside this app; it is listed but never modified.",
};

const pad = (value: number) => String(value).padStart(2, "0");
function cadenceLabel(cadence: ScheduleCadence): string {
  const time = `${pad(cadence.hour)}:${pad(cadence.minute)}`;
  return cadence.kind === "daily" ? `Every day at ${time}` : `Every ${weekdayLabel(cadence.day)} at ${time}`;
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
      setError(errorText(reason, "Scheduled scans could not be read."));
    } finally {
      setLoading(false);
    }
  }, []);
  useEffect(() => { void refresh(); }, [refresh]);

  const addSchedule = () => {
    const parsed = parseTime(time);
    if (!parsed) { setFormError("Enter a time as HH:MM."); return; }
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
    if (change.kind === "upsertScanSchedule") return `Add scheduled scan: ${cadenceLabel(change.cadence)}`;
    if (change.kind === "removeScanSchedule") {
      const cadence = schedules?.find((schedule) => schedule.id === change.scheduleId)?.cadence;
      return `Remove scheduled scan: ${cadence ? cadenceLabel(cadence) : change.scheduleId}`;
    }
    return change.kind;
  }, [schedules]);

  const toggleRemoval = (id: string) => setRemovals((current) => current.includes(id) ? current.filter((value) => value !== id) : [...current, id]);
  const onFinished = useCallback(() => { setJournalKey((value) => value + 1); void refresh(); }, [refresh]);

  return <section className="scheduler-page" aria-labelledby="scheduler-title">
    <header className="page-header"><div>
      <p className="eyebrow">System</p>
      <h1 id="scheduler-title">Scheduled scans</h1>
      <p>Scheduled scans are read-only: they run a cleanup preview through Windows Task Scheduler and record a summary, and they never delete anything.</p>
    </div></header>
    <button type="button" disabled={loading} onClick={() => void refresh()}>Refresh</button>
    {loading && <p role="status">Reading scheduled scans…</p>}
    {error && <p role="alert">{error}</p>}

    <form className="scheduler-form" aria-label="New scheduled scan" onSubmit={(event) => { event.preventDefault(); addSchedule(); }}>
      <label>Frequency
        <select value={frequency} onChange={(event) => setFrequency(event.target.value === "weekly" ? "weekly" : "daily")}>
          <option value="daily">Daily</option>
          <option value="weekly">Weekly</option>
        </select>
      </label>
      {frequency === "weekly" && <label>Weekday
        <select value={day} onChange={(event) => setDay(event.target.value as Weekday)}>
          {weekdays.map((entry) => <option key={entry.value} value={entry.value}>{entry.label}</option>)}
        </select>
      </label>}
      <label>Time
        <input type="time" value={time} required onChange={(event) => setTime(event.target.value)} />
      </label>
      <button type="submit">Add schedule</button>
      {formError && <p role="alert">{formError}</p>}
    </form>

    {pending.length > 0 && <>
      <h2>New schedules to review</h2>
      <ul className="scheduler-pending">{pending.map((change) => <li key={change.scheduleId}>
        {cadenceLabel(change.cadence)}{" "}
        <button type="button" onClick={() => setPending((current) => current.filter((entry) => entry.scheduleId !== change.scheduleId))}
          aria-label={`Discard new schedule: ${cadenceLabel(change.cadence)}`}>Discard</button>
      </li>)}</ul>
    </>}

    <h2>Existing schedules</h2>
    {schedules && schedules.length === 0 && <p>No scheduled scans.</p>}
    {schedules && schedules.length > 0 && <ul className="scheduler-list">{schedules.map((schedule) => {
      const removable = schedule.cadence !== null;
      return <li key={schedule.id}>
        <strong>{schedule.cadence ? cadenceLabel(schedule.cadence) : "Unrecognized schedule"}</strong>
        <p>{schedule.enabled ? "Enabled" : "Disabled in Task Scheduler"} · Last run: {schedule.lastRun ?? "Never"} · Next run: {schedule.nextRun ?? "Not scheduled"}</p>
        {schedule.orphaned && <p>Orphaned: {schedule.orphanReason ? orphanLabel[schedule.orphanReason] : "The task no longer matches this app."}</p>}
        {!removable && <p>This task cannot be removed here because its definition is not recognized.</p>}
        <label>
          <input type="checkbox" disabled={!removable} checked={removals.includes(schedule.id)} onChange={() => toggleRemoval(schedule.id)} />
          Remove schedule
        </label>
      </li>;
    })}</ul>}

    <SystemChangeReview changes={changes} describe={describe} onFinished={onFinished} />

    <h2>Recent scheduled scan results</h2>
    {summaries.length === 0
      ? <p>No scheduled scan has finished yet.</p>
      : <ul className="scheduler-summaries">{summaries.map((summary) => <li key={`${summary.scheduleId}:${summary.finishedAt}`}>
        {new Date(summary.finishedAt * 1000).toLocaleString()}: {summary.succeeded
          ? `${formatBytes(summary.reclaimableBytes)} reclaimable in ${summary.itemCount.toLocaleString()} items, ${summary.diagnosticCount.toLocaleString()} diagnostics`
          : "Scan did not finish"}
      </li>)}</ul>}

    <SystemChangeJournal module="scheduler" describe={describe} refreshKey={journalKey} onFinished={onFinished} />
  </section>;
}
