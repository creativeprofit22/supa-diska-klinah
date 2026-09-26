// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { SystemChangeActions } from "./api";
import { SystemChangeJournal } from "./SystemChangeJournal";
import { SystemChangeReview } from "./SystemChangeReview";
import type { ExecutionReport, JournalView, PlanTicket, SystemChange } from "./types";

afterEach(cleanup);
const hibernateOff: SystemChange = { kind: "setHibernation", enabled: false };
const driver: SystemChange = { kind: "deleteDriverPackage", publishedName: "oem12.inf" };
const describe = (change: SystemChange) => change.kind;
const ticket: PlanTicket = {
  planId: "a".repeat(32), requiresHelper: true, expiresInSeconds: 60,
  changes: [
    { change: hibernateOff, module: "power", privilege: "helper", reversibility: { kind: "reversible" }, impact: { component: "Hibernation", effect: "Turns hibernation off", restart: "none", risk: "low" }, expectedPrior: { kind: "enabled", enabled: true }, inverse: { kind: "setHibernation", enabled: true } },
    { change: driver, module: "drivers", privilege: "helper", reversibility: { kind: "irreversible", reason: "the driver package is removed from the store" }, impact: { component: "Driver package oem12.inf", effect: "Removes an unused driver", restart: "none", risk: "high" }, expectedPrior: { kind: "driverPackage", present: true }, inverse: null },
  ],
};
const partial: ExecutionReport = { planId: ticket.planId, results: [
  { change: hibernateOff, outcome: { status: "applied" }, journalEntryId: "b".repeat(32) },
  { change: driver, outcome: { status: "denied" }, journalEntryId: "c".repeat(32) },
] };
function fakeActions(overrides: Partial<SystemChangeActions> = {}): SystemChangeActions {
  return {
    preview: vi.fn(), createPlan: vi.fn(async () => ticket), confirm: vi.fn(async () => undefined),
    execute: vi.fn(async () => partial), journal: vi.fn(async () => []), createRollbackPlan: vi.fn(async () => ticket),
    ...overrides,
  };
}

it("lists impact and reversibility before asking Windows to confirm, and reports partial failure", async () => {
  const actions = fakeActions();
  render(<SystemChangeReview changes={[hibernateOff, driver]} describe={describe} actions={actions} />);
  fireEvent.click(screen.getByRole("button", { name: "Review selected changes (2)" }));
  await screen.findByText("Turns hibernation off");
  expect(screen.getByText(/Cannot be undone: the driver package is removed/)).toBeTruthy();
  expect(screen.getByText(/administrator permission \(UAC\) will be requested once/)).toBeTruthy();
  expect(actions.confirm).not.toHaveBeenCalled();
  expect(actions.execute).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Continue to Windows confirmation" }));
  expect((await screen.findByRole("alert")).textContent).toMatch(/Some changes did not finish/);
  expect(screen.getByText(/Denied: administrator permission was not granted/)).toBeTruthy();
  expect(actions.confirm).toHaveBeenCalledWith(ticket.planId);
  expect(actions.execute).toHaveBeenCalledTimes(1);
});

it("never executes when the Windows confirmation is declined", async () => {
  const actions = fakeActions({ confirm: vi.fn(async () => { throw { code: "confirmationDeclined", message: "x" }; }) });
  render(<SystemChangeReview changes={[hibernateOff]} describe={describe} actions={actions} />);
  fireEvent.click(screen.getByRole("button", { name: /Review selected changes/ }));
  fireEvent.click(await screen.findByRole("button", { name: "Continue to Windows confirmation" }));
  expect((await screen.findByRole("alert")).textContent).toMatch(/Nothing was changed/);
  expect(actions.execute).not.toHaveBeenCalled();
});

it("explains an unreadable change history instead of showing the raw backend text", async () => {
  const actions = fakeActions({ createPlan: vi.fn(async () => { throw { code: "journalUnavailable", message: "the change journal is unavailable" }; }) });
  render(<SystemChangeReview changes={[hibernateOff]} describe={describe} actions={actions} />);
  fireEvent.click(screen.getByRole("button", { name: /Review selected changes/ }));
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toBe("The change history could not be read. It may have been written by a newer version of this app; update the app, or see the administrator guide to reset it.");
  expect(actions.confirm).not.toHaveBeenCalled();
});

it("refuses more than 32 changes before contacting the backend", () => {
  const actions = fakeActions();
  const changes = Array.from({ length: 33 }, (_, i): SystemChange => ({ kind: "setHibernation", enabled: i % 2 === 0 }));
  render(<SystemChangeReview changes={changes} describe={describe} actions={actions} />);
  expect((screen.getByRole("button", { name: /Review selected changes/ }) as HTMLButtonElement).disabled).toBe(true);
  expect(actions.createPlan).not.toHaveBeenCalled();
});

it("offers undo only for journal entries that can be rolled back", async () => {
  const entry = (id: string, change: SystemChange): JournalView["entry"] => ({ id, planId: "p".repeat(32), recordedAt: 1, change, prior: null, reversibility: { kind: "reversible" }, inverse: null, outcome: { status: "applied" }, rolledBackBy: null });
  const views: JournalView[] = [
    { entry: entry("1".repeat(32), hibernateOff), rollback: "available" },
    { entry: { ...entry("2".repeat(32), driver), reversibility: { kind: "irreversible", reason: "gone" } }, rollback: "irreversible" },
  ];
  const actions = fakeActions({ journal: vi.fn(async () => views) });
  render(<SystemChangeJournal describe={describe} actions={actions} />);
  const boxes = await screen.findAllByRole("checkbox") as HTMLInputElement[];
  expect(boxes[0].disabled).toBe(false);
  expect(boxes[1].disabled).toBe(true);
  fireEvent.click(boxes[0]);
  fireEvent.click(screen.getByRole("button", { name: "Review undo (1)" }));
  await screen.findByText("Turns hibernation off");
  expect(actions.createRollbackPlan).toHaveBeenCalledWith(["1".repeat(32)]);
  expect(actions.execute).not.toHaveBeenCalled();
});

it("calls onFinished exactly once with the report when an undo completes", async () => {
  const view: JournalView = { entry: { id: "1".repeat(32), planId: "p".repeat(32), recordedAt: 1, change: hibernateOff, prior: null, reversibility: { kind: "reversible" }, inverse: null, outcome: { status: "applied" }, rolledBackBy: null }, rollback: "available" };
  const actions = fakeActions({ journal: vi.fn(async () => [view]) });
  const onFinished = vi.fn();
  // Mirrors a page: the callback bumps refreshKey and is a fresh function on every render.
  function Page() {
    const [key, setKey] = useState(0);
    return <SystemChangeJournal describe={describe} actions={actions} refreshKey={key} onFinished={(report) => { onFinished(report); setKey((n) => n + 1); }} />;
  }
  render(<Page />);
  fireEvent.click(await screen.findByRole("checkbox"));
  fireEvent.click(screen.getByRole("button", { name: "Review undo (1)" }));
  fireEvent.click(await screen.findByRole("button", { name: "Continue to Windows confirmation" }));
  await screen.findByLabelText("System change results");
  await waitFor(() => expect(onFinished).toHaveBeenCalledTimes(1));
  expect(onFinished).toHaveBeenCalledWith(partial);
  await new Promise((resolve) => setTimeout(resolve, 20));
  expect(onFinished).toHaveBeenCalledTimes(1);
});
