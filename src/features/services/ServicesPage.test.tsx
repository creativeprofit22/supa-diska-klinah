// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { PlanTicket, SystemChange } from "../../shared/system-change/types";
import { I18nProvider } from "../../shared/i18n/I18nProvider";
import { ServicesPage } from "./ServicesPage";
import type { ServiceItem } from "./types";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const base = { risk: "low", category: "telemetry", recommended: "disabled", installed: true, delayedAutoStart: false, running: true } as const;
const items: ServiceItem[] = [
  { ...base, id: "diagtrack", serviceName: "DiagTrack", label: "Connected User Experiences", description: "Sends diagnostic data.", start: "automatic" },
  { ...base, id: "xblgamesave", serviceName: "XblGameSave", label: "Xbox Live Game Save", description: "Syncs game saves.", category: "gaming", recommended: "manual", start: "manual", running: false },
  { ...base, id: "fax", serviceName: "Fax", label: "Fax", description: "Sends faxes.", category: "legacy", installed: false, start: null, running: false },
  { ...base, id: "bootdrv", serviceName: "BootDrv", label: "Boot driver", description: "Boot-start.", risk: "high", start: "boot" },
];
const calls = (name: string) => invoke.mock.calls.filter(([command]) => command === name);
const select = (label: string) => screen.getByRole("combobox", { name: `Start type for ${label}` }) as HTMLSelectElement;

beforeEach(() => {
  invoke.mockImplementation(async (command: string, bytes?: Uint8Array) => {
    if (command === "list_services") return items;
    if (command === "system_change_journal") return [];
    if (command === "create_system_change_plan") {
      const { changes } = JSON.parse(new TextDecoder().decode(bytes)) as { changes: SystemChange[] };
      return {
        planId: "a".repeat(32), requiresHelper: true, expiresInSeconds: 60,
        changes: changes.map((change) => ({
          change, module: "services", privilege: "helper", reversibility: { kind: "reversible" },
          impact: { component: "Service", effect: "Start type changes", restart: "none", risk: "low" }, expectedPrior: null, inverse: null,
        })),
      } satisfies PlanTicket;
    }
    throw Error(`Unexpected command ${command}`);
  });
});
afterEach(() => { cleanup(); vi.clearAllMocks(); });

it("renders services with risk, description, recommendation and nothing selected on load", async () => {
  render(<ServicesPage />);
  expect(await screen.findByText("Sends diagnostic data.")).toBeTruthy();
  expect(screen.getAllByText(/Recommended: Disabled/).length).toBeGreaterThan(0);
  expect(screen.getAllByText(/Low risk/).length).toBeGreaterThan(0);
  for (const box of screen.getAllByRole("combobox")) expect((box as HTMLSelectElement).value).toBe("");
  expect((screen.getByRole("button", { name: "Review selected changes (0)" }) as HTMLButtonElement).disabled).toBe(true);
  expect(calls("create_system_change_plan")).toHaveLength(0);
});

it("choosing Disabled on one service sends exactly that change and does not confirm before the Windows step", async () => {
  render(<ServicesPage />);
  await screen.findByText("Sends diagnostic data.");
  fireEvent.change(select("Connected User Experiences"), { target: { value: "disabled" } });
  fireEvent.click(screen.getByRole("button", { name: "Review selected changes (1)" }));
  await screen.findByRole("button", { name: "Continue to Windows confirmation" });
  const sent = calls("create_system_change_plan");
  expect(sent).toHaveLength(1);
  expect(JSON.parse(new TextDecoder().decode(sent[0][1] as Uint8Array))).toEqual({
    changes: [{ kind: "setServiceStartType", catalogId: "diagtrack", startType: "disabled" }],
  });
  await waitFor(() => expect(calls("system_change_journal").length).toBeGreaterThan(0));
  expect(calls("confirm_system_change_plan")).toHaveLength(0);
  expect(calls("execute_system_change_plan")).toHaveLength(0);
});

it("choosing the current start type produces no change", async () => {
  render(<ServicesPage />);
  await screen.findByText("Syncs game saves.");
  fireEvent.change(select("Xbox Live Game Save"), { target: { value: "manual" } });
  expect(screen.getByRole("button", { name: "Review selected changes (0)" })).toBeTruthy();
});

it("disables not-installed and boot/system services with a reason", async () => {
  render(<ServicesPage />);
  await screen.findByText("Sends faxes.");
  expect(select("Fax").disabled).toBe(true);
  expect(select("Boot driver").disabled).toBe(true);
  expect(select("Connected User Experiences").disabled).toBe(false);
  expect(screen.getByText(/Not installed on this PC/)).toBeTruthy();
  expect(screen.getByText(/boot or system driver start type/)).toBeTruthy();
});

it("renders in Latin American Spanish", async () => {
  render(<I18nProvider languages={["es-MX"]}><ServicesPage /></I18nProvider>);
  expect((await screen.findAllByRole("option", { name: "Mantener el actual" })).length).toBeGreaterThan(0);
  expect(screen.getByRole("heading", { level: 1, name: "Servicios de Windows" })).toBeTruthy();
});
