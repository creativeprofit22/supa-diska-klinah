// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { PlanTicket, SystemChange } from "../../shared/system-change/types";
import { StartupPage } from "./StartupPage";
import type { StartupItem } from "./types";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const items: StartupItem[] = [
  { name: "Chat", scope: "user", location: "run", source: "run", command: "chat.exe --tray", enabled: true, toggleable: true },
  { name: "Updater", scope: "machine", location: "run32", source: "run32", command: "upd.exe", enabled: false, toggleable: true },
  { name: "OneShot", scope: "user", location: null, source: "runOnce", command: "setup.exe /finish", enabled: true, toggleable: false },
  { name: "\Vendor\Logon", scope: "machine", location: null, source: "logonTask", command: "", enabled: true, toggleable: false },
];
const calls = (name: string) => invoke.mock.calls.filter(([command]) => command === name);

beforeEach(() => {
  invoke.mockImplementation(async (command: string, bytes?: Uint8Array) => {
    if (command === "list_startup_items") return items;
    if (command === "system_change_journal") return [];
    if (command === "create_system_change_plan") {
      const { changes } = JSON.parse(new TextDecoder().decode(bytes)) as { changes: SystemChange[] };
      return {
        planId: "a".repeat(32), requiresHelper: false, expiresInSeconds: 60,
        changes: changes.map((change) => ({
          change, module: "startup", privilege: "standard", reversibility: { kind: "reversible" },
          impact: { component: "Startup item", effect: "Stops starting", restart: "none", risk: "low" }, expectedPrior: null, inverse: null,
        })),
      } satisfies PlanTicket;
    }
    throw Error(`Unexpected command ${command}`);
  });
});
afterEach(() => { cleanup(); vi.clearAllMocks(); });

it("renders the inventory with nothing selected or sent on load", async () => {
  render(<StartupPage />);
  expect(await screen.findByText("Chat")).toBeTruthy();
  expect(screen.getByText(/Deleting startup entries is not offered/)).toBeTruthy();
  for (const box of screen.getAllByRole("checkbox")) expect((box as HTMLInputElement).checked).toBe(false);
  expect((screen.getByRole("button", { name: "Review selected changes (0)" }) as HTMLButtonElement).disabled).toBe(true);
  expect(calls("create_system_change_plan")).toHaveLength(0);
});

it("toggling a user-scope item sends exactly that entry and never confirms before the Windows step", async () => {
  render(<StartupPage />);
  fireEvent.click(await screen.findByRole("checkbox", { name: "Change Chat to disabled" }));
  fireEvent.click(screen.getByRole("button", { name: "Review selected changes (1)" }));
  await screen.findByRole("button", { name: "Continue to Windows confirmation" });
  const sent = calls("create_system_change_plan");
  expect(sent).toHaveLength(1);
  expect(JSON.parse(new TextDecoder().decode(sent[0][1] as Uint8Array))).toEqual({
    changes: [{ kind: "setStartupEntry", entry: { scope: "user", location: "run", name: "Chat" }, enabled: false }],
  });
  await waitFor(() => expect(calls("system_change_journal").length).toBeGreaterThan(0));
  expect(calls("confirm_system_change_plan")).toHaveLength(0);
  expect(calls("execute_system_change_plan")).toHaveLength(0);
});

it("offers re-enabling a disabled item", async () => {
  render(<StartupPage />);
  expect(await screen.findByRole("checkbox", { name: "Change Updater to enabled" })).toBeTruthy();
});

it("shows read-only items with a reason and no enabled checkbox", async () => {
  render(<StartupPage />);
  await screen.findByText("OneShot");
  expect(screen.getAllByRole("checkbox")).toHaveLength(2);
  expect(screen.queryByRole("checkbox", { name: /OneShot/ })).toBeNull();
  expect(screen.queryByRole("checkbox", { name: /Logon/ })).toBeNull();
  expect(screen.getByText(/RunOnce entries run a single time/)).toBeTruthy();
  expect(screen.getByText(/Manage it in Task Scheduler/)).toBeTruthy();
});

it("reloads the startup inventory after an undo from Change history completes", async () => {
  const change: SystemChange = { kind: "setStartupEntry", entry: { scope: "user", location: "run", name: "Chat" }, enabled: false };
  const entryId = "1".repeat(32);
  const base = invoke.getMockImplementation()!;
  invoke.mockImplementation(async (command: string, bytes?: Uint8Array) => {
    if (command === "system_change_journal") return [{
      entry: { id: entryId, planId: "p".repeat(32), recordedAt: 1, change, prior: null, reversibility: { kind: "reversible" }, inverse: null, outcome: { status: "applied" }, rolledBackBy: null },
      rollback: "available",
    }];
    if (command === "create_system_rollback_plan") return {
      planId: "b".repeat(32), requiresHelper: false, expiresInSeconds: 60,
      changes: [{ change: { ...change, enabled: true }, module: "startup", privilege: "standard", reversibility: { kind: "reversible" },
        impact: { component: "Startup item", effect: "Starts again", restart: "none", risk: "low" }, expectedPrior: null, inverse: null }],
    } satisfies PlanTicket;
    if (command === "confirm_system_change_plan") return undefined;
    if (command === "execute_system_change_plan") return { planId: "b".repeat(32), results: [{ change: { ...change, enabled: true }, outcome: { status: "applied" }, journalEntryId: "2".repeat(32) }] };
    return base(command, bytes);
  });
  render(<StartupPage />);
  await screen.findByText("Chat");
  fireEvent.click(await screen.findByRole("checkbox", { name: "Disable startup item: Chat" }));
  fireEvent.click(screen.getByRole("button", { name: "Review undo (1)" }));
  fireEvent.click(await screen.findByRole("button", { name: "Continue to Windows confirmation" }));
  await waitFor(() => expect(calls("execute_system_change_plan")).toHaveLength(1));
  expect(JSON.parse(new TextDecoder().decode(calls("create_system_rollback_plan")[0][1] as Uint8Array))).toEqual({ entryIds: [entryId] });
  await waitFor(() => expect(calls("list_startup_items")).toHaveLength(2));
});
