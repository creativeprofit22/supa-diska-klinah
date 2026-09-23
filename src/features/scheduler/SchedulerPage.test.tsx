// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { PlanTicket, SystemChange } from "../../shared/system-change/types";
import { SchedulerPage } from "./SchedulerPage";
import type { ScanSchedule, ScheduledScanSummary } from "./types";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const decode = (bytes: unknown) => JSON.parse(new TextDecoder().decode(bytes as Uint8Array)) as { changes: SystemChange[] };
const uuid = (n: number) => `0000000${n}-0000-4000-8000-000000000000`;
const schedules: ScanSchedule[] = [
  { id: uuid(1), cadence: { kind: "daily", hour: 3, minute: 0 }, enabled: true, lastRun: null, nextRun: "2026-09-24T03:00:00", orphaned: false, orphanReason: null },
  { id: uuid(2), cadence: { kind: "weekly", day: "friday", hour: 22, minute: 15 }, enabled: true, lastRun: null, nextRun: null, orphaned: true, orphanReason: "foreignExecutable" },
  { id: uuid(3), cadence: null, enabled: false, lastRun: null, nextRun: null, orphaned: true, orphanReason: "unrecognizedDefinition" },
];
const summaries: ScheduledScanSummary[] = [
  { scheduleId: uuid(1), finishedAt: 1_700_000_000, reclaimableBytes: 2048, itemCount: 4, diagnosticCount: 1, succeeded: true },
];
const calls = (name: string) => invoke.mock.calls.filter(([command]) => command === name);

beforeEach(() => {
  invoke.mockImplementation(async (command: string, bytes?: unknown) => {
    if (command === "list_scan_schedules") return schedules;
    if (command === "list_scheduled_scan_summaries") return summaries;
    if (command === "system_change_journal") return [];
    if (command === "create_system_change_plan") {
      const { changes } = decode(bytes);
      return {
        planId: "a".repeat(32), requiresHelper: false, expiresInSeconds: 60,
        changes: changes.map((change) => ({
          change, module: "scheduler", privilege: "standard", reversibility: { kind: "reversible" },
          impact: { component: "Scheduled scan (Task Scheduler)", effect: "Change schedule", restart: "none", risk: "low" },
          expectedPrior: null, inverse: null,
        })),
      } satisfies PlanTicket;
    }
    throw Error(`Forbidden command ${command}`);
  });
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.clearAllMocks(); });

const rowFor = (text: string) => screen.getByText(text).closest("li") as HTMLElement;

it("lists schedules and summaries, explains read-only scans, and sends nothing on load", async () => {
  render(<SchedulerPage />);
  expect(await screen.findByText("Every day at 03:00")).toBeTruthy();
  expect(screen.getByText(/never delete anything/)).toBeTruthy();
  expect(screen.getByText(/2 KB reclaimable in 4 items/)).toBeTruthy();
  for (const box of screen.getAllByRole("checkbox", { name: "Remove schedule" })) expect((box as HTMLInputElement).checked).toBe(false);
  expect(calls("create_system_change_plan")).toHaveLength(0);
});

it("shows the orphan reason and disables removal of an unrecognized task", async () => {
  render(<SchedulerPage />);
  await screen.findByText("Every Friday at 22:15");
  expect(within(rowFor("Every Friday at 22:15")).getByText(/runs a different program than this installation/)).toBeTruthy();
  const unrecognized = rowFor("Unrecognized schedule");
  expect(within(unrecognized).getByText(/listed but never modified/)).toBeTruthy();
  expect((within(unrecognized).getByRole("checkbox", { name: "Remove schedule" }) as HTMLInputElement).disabled).toBe(true);
});

it("adds a weekly schedule with the exact cadence and a lowercase id", async () => {
  vi.spyOn(crypto, "randomUUID").mockReturnValue("ABCDEF01-2345-4678-89AB-CDEF01234567");
  render(<SchedulerPage />);
  await screen.findByText("Every day at 03:00");
  fireEvent.change(screen.getByLabelText("Frequency"), { target: { value: "weekly" } });
  fireEvent.change(screen.getByLabelText("Weekday"), { target: { value: "sunday" } });
  fireEvent.change(screen.getByLabelText("Time"), { target: { value: "07:30" } });
  expect(calls("create_system_change_plan")).toHaveLength(0);
  fireEvent.click(screen.getByRole("button", { name: "Add schedule" }));
  fireEvent.click(screen.getByRole("button", { name: "Review selected changes (1)" }));
  await screen.findByRole("button", { name: "Continue to Windows confirmation" });
  expect(decode(calls("create_system_change_plan")[0][1])).toEqual({
    changes: [{ kind: "upsertScanSchedule", scheduleId: "abcdef01-2345-4678-89ab-cdef01234567", cadence: { kind: "weekly", day: "sunday", hour: 7, minute: 30 } }],
  });
  expect(calls("confirm_system_change_plan")).toHaveLength(0);
  expect(calls("execute_system_change_plan")).toHaveLength(0);
});

it("removes exactly the selected schedule id", async () => {
  render(<SchedulerPage />);
  await screen.findByText("Every day at 03:00");
  fireEvent.click(within(rowFor("Every day at 03:00")).getByRole("checkbox", { name: "Remove schedule" }));
  fireEvent.click(screen.getByRole("button", { name: "Review selected changes (1)" }));
  await screen.findByRole("button", { name: "Continue to Windows confirmation" });
  expect(decode(calls("create_system_change_plan")[0][1])).toEqual({ changes: [{ kind: "removeScanSchedule", scheduleId: uuid(1) }] });
  expect(calls("confirm_system_change_plan")).toHaveLength(0);
  expect(calls("execute_system_change_plan")).toHaveLength(0);
});
