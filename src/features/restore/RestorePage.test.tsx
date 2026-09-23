// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { PlanTicket, SystemChange } from "../../shared/system-change/types";
import { descriptionError, MAX_DESCRIPTION, RestorePage } from "./RestorePage";
import type { RestorePointList, RestoreProtection } from "./types";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const available: RestorePointList = { status: "available", points: [
  { sequenceNumber: 7, description: "Installed Fixture App", createdAt: 1_700_000_000, restorePointType: 0, kind: "applicationInstall" },
] };
const protection: RestoreProtection = { policyDisabled: false, protectionEnabled: true, creationFrequencyMinutes: 1440 };
let points: RestorePointList = available;
const decode = (bytes: unknown) => JSON.parse(new TextDecoder().decode(bytes as Uint8Array)) as { changes: SystemChange[] };
const called = (name: string) => invoke.mock.calls.filter(([command]) => command === name);

beforeEach(() => {
  points = available;
  invoke.mockImplementation(async (command: string, bytes?: Uint8Array) => {
    if (command === "list_restore_points") return points;
    if (command === "get_restore_protection") return protection;
    if (command === "system_change_journal") return [];
    if (command === "create_system_change_plan") {
      const { changes } = decode(bytes);
      return {
        planId: "a".repeat(32), requiresHelper: true, expiresInSeconds: 60,
        changes: changes.map((change) => ({
          change, module: "restore", privilege: "helper", expectedPrior: null, inverse: null,
          reversibility: { kind: "irreversible", reason: "restore point" },
          impact: { component: "System Restore", effect: "Creates a restore point", restart: "none", risk: "low" },
        })),
      } satisfies PlanTicket;
    }
    throw Error(`Unexpected command ${command}`);
  });
});
afterEach(() => { cleanup(); vi.clearAllMocks(); });

it("renders protection and points without sending anything on load", async () => {
  render(<RestorePage />);
  await screen.findByText("Installed Fixture App");
  expect(screen.getByText("System Restore is not disabled by policy.")).toBeTruthy();
  expect(screen.getByText("System drive protection is on.")).toBeTruthy();
  expect(screen.getByText(/1440 minutes\. Windows may skip a new restore point/)).toBeTruthy();
  expect((screen.getByLabelText("Description") as HTMLInputElement).value).toBe("Supa Diska Klinah manual restore point");
  expect((screen.getByRole("button", { name: "Review selected changes (0)" }) as HTMLButtonElement).disabled).toBe(true);
  expect(called("create_system_change_plan")).toHaveLength(0);
});

it("explains when listing requires administrator permission", async () => {
  points = { status: "requiresAdministrator", points: [] };
  render(<RestorePage />);
  expect(await screen.findByText(/Listing restore points requires administrator permission/)).toBeTruthy();
});

it("sends the exact description and does not confirm or execute before Windows confirmation", async () => {
  render(<RestorePage />);
  await screen.findByText("Installed Fixture App");
  fireEvent.change(screen.getByLabelText("Description"), { target: { value: "Before tuning — ünïcode" } });
  fireEvent.click(screen.getByRole("button", { name: "Add restore point to review" }));
  fireEvent.click(screen.getByRole("button", { name: "Review selected changes (1)" }));
  await screen.findByRole("button", { name: "Continue to Windows confirmation" });
  const calls = called("create_system_change_plan");
  expect(calls).toHaveLength(1);
  expect(decode(calls[0][1])).toEqual({ changes: [{ kind: "createRestorePoint", description: "Before tuning — ünïcode" }] });
  expect(called("confirm_system_change_plan")).toHaveLength(0);
  expect(called("execute_system_change_plan")).toHaveLength(0);
});

it.each([["empty", "   "], ["too long", "x".repeat(129)], ["control character", "tab\there"]])("disables review for a %s description", async (_label, value) => {
  render(<RestorePage />);
  await screen.findByText("Installed Fixture App");
  fireEvent.click(screen.getByRole("button", { name: "Add restore point to review" }));
  expect(screen.getByRole("button", { name: "Review selected changes (1)" })).toBeTruthy();
  fireEvent.change(screen.getByLabelText("Description"), { target: { value } });
  expect(screen.getByRole("alert")).toBeTruthy();
  expect((screen.getByRole("button", { name: "Add restore point to review" }) as HTMLButtonElement).disabled).toBe(true);
  expect((screen.getByRole("button", { name: "Review selected changes (0)" }) as HTMLButtonElement).disabled).toBe(true);
  expect(called("create_system_change_plan")).toHaveLength(0);
});

it("uses the backend limit of 128 UTF-16 code units", () => {
  expect(MAX_DESCRIPTION).toBe(128);
  expect(descriptionError("x".repeat(128))).toBeNull();
  expect(descriptionError("x".repeat(129))).toBe("Use at most 128 characters.");
  // A surrogate pair counts as two code units, as in Rust's encode_utf16().count().
  expect(descriptionError("x".repeat(126) + "\u{1F600}")).toBeNull();
  expect(descriptionError("x".repeat(127) + "\u{1F600}")).not.toBeNull();
});

it("accepts and sends a 128-character description", async () => {
  const value = "x".repeat(128);
  render(<RestorePage />);
  await screen.findByText("Installed Fixture App");
  const input = screen.getByLabelText("Description") as HTMLInputElement;
  expect(input.maxLength).toBe(128);
  fireEvent.change(input, { target: { value } });
  expect(screen.queryByRole("alert")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Add restore point to review" }));
  fireEvent.click(screen.getByRole("button", { name: "Review selected changes (1)" }));
  await screen.findByRole("button", { name: "Continue to Windows confirmation" });
  const calls = called("create_system_change_plan");
  expect(calls).toHaveLength(1);
  expect(decode(calls[0][1])).toEqual({ changes: [{ kind: "createRestorePoint", description: value }] });
});
