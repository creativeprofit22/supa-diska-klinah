// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { PlanTicket, SystemChange } from "../../shared/system-change/types";
import { PowerPage } from "./PowerPage";
import type { PowerStatus } from "./types";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const balanced = "381b4222-f694-41f0-9685-ff5bb260df2e";
const high = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";
const supported: PowerStatus = {
  hibernation: { supported: true, enabled: true, hiberfileBytes: 6 * 1024 * 1024 * 1024 },
  schemes: [{ id: balanced, name: "Balanced", active: true }, { id: high, name: "High performance", active: false }],
};
let fixture: PowerStatus = supported;

const calls = (name: string) => invoke.mock.calls.filter(([command]) => command === name);
const sent = () => calls("create_system_change_plan").map(([, bytes]) => (JSON.parse(new TextDecoder().decode(bytes as Uint8Array)) as { changes: SystemChange[] }).changes);

beforeEach(() => {
  fixture = supported;
  invoke.mockImplementation(async (command: string, bytes?: Uint8Array) => {
    if (command === "get_power_status") { expect(bytes).toBeUndefined(); return fixture; }
    if (command === "system_change_journal") return [];
    if (command === "create_system_change_plan") {
      const { changes } = JSON.parse(new TextDecoder().decode(bytes)) as { changes: SystemChange[] };
      return {
        planId: "a".repeat(32), requiresHelper: true, expiresInSeconds: 60,
        changes: changes.map((change) => ({ change, module: "power", privilege: "helper", reversibility: { kind: "reversible" }, impact: { component: "Power", effect: "Change", restart: "none", risk: "low" }, expectedPrior: null, inverse: null })),
      } satisfies PlanTicket;
    }
    throw Error(`Unexpected command ${command}`);
  });
});
afterEach(() => { cleanup(); vi.clearAllMocks(); });

it("renders status with nothing selected or sent on load", async () => {
  render(<PowerPage />);
  expect(await screen.findByText(/6 GB/)).toBeTruthy();
  expect(screen.getByText(/also disables Fast Startup/)).toBeTruthy();
  expect((screen.getByRole("checkbox", { name: "Turn hibernation off" }) as HTMLInputElement).checked).toBe(false);
  expect((screen.getByRole("radio", { name: /Balanced/ }) as HTMLInputElement).checked).toBe(true);
  expect((screen.getByRole("radio", { name: /High performance/ }) as HTMLInputElement).checked).toBe(false);
  expect((screen.getByRole("button", { name: "Review selected changes (0)" }) as HTMLButtonElement).disabled).toBe(true);
  expect(calls("create_system_change_plan")).toHaveLength(0);
});

it("sends the exact hibernation change and waits for Windows confirmation", async () => {
  render(<PowerPage />);
  fireEvent.click(await screen.findByRole("checkbox", { name: "Turn hibernation off" }));
  fireEvent.click(screen.getByRole("button", { name: "Review selected changes (1)" }));
  await screen.findByRole("button", { name: "Continue to Windows confirmation" });
  expect(sent()).toEqual([[{ kind: "setHibernation", enabled: false }]]);
  expect(calls("confirm_system_change_plan")).toHaveLength(0);
  expect(calls("execute_system_change_plan")).toHaveLength(0);
});

it("sends the exact scheme id when a different plan is picked", async () => {
  render(<PowerPage />);
  fireEvent.click(await screen.findByRole("radio", { name: /High performance/ }));
  fireEvent.click(screen.getByRole("button", { name: "Review selected changes (1)" }));
  await screen.findByRole("button", { name: "Continue to Windows confirmation" });
  expect(sent()).toEqual([[{ kind: "setActivePowerScheme", scheme: high }]]);
});

it("shows the reason and no checkbox when hibernation is unsupported", async () => {
  fixture = { ...supported, hibernation: { supported: false, enabled: false, hiberfileBytes: null } };
  render(<PowerPage />);
  expect(await screen.findByText(/Hibernation is not supported on this device/)).toBeTruthy();
  expect(screen.queryByRole("checkbox")).toBeNull();
});
