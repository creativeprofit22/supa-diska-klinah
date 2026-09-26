// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { I18nProvider } from "../../shared/i18n/I18nProvider";
import { OptimizerPage } from "./OptimizerPage";
import type { OptimizerReport, Proposal } from "./types";
import type { PlannedChange, SystemChange } from "../../shared/system-change/types";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

// Optimizer proposals are owned by the module of their change; there is no "optimizer" module.
const moduleOf: Partial<Record<SystemChange["kind"], string>> = {
  setServiceStartType: "services", setUserSetting: "privacy", setActivePowerScheme: "power",
};
const planned = (change: SystemChange, privilege: "standard" | "helper", effect: string): PlannedChange => ({
  change, module: moduleOf[change.kind] ?? "services", privilege, reversibility: { kind: "reversible" },
  impact: { component: effect, effect, restart: "none", risk: "low" }, expectedPrior: null, inverse: null,
});
const telemetry: SystemChange = { kind: "setServiceStartType", catalogId: "diagtrack", startType: "disabled" };
const xbox: SystemChange = { kind: "setServiceStartType", catalogId: "xblgamesave", startType: "manual" };
const ads: SystemChange = { kind: "setUserSetting", settingId: "advertising-id", value: 0 };
const power: SystemChange = { kind: "setActivePowerScheme", scheme: "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c" };
const proposal = (group: Proposal["group"], label: string, suggested: boolean, change: SystemChange, privilege: "standard" | "helper" = "helper"): Proposal =>
  ({ group, label, suggested, planned: planned(change, privilege, `Effect of ${label}`) });
// Report order differs from display (group) order on purpose.
const report: OptimizerReport = {
  proposals: [
    proposal("privacy", "Turn off advertising ID", true, ads, "standard"),
    proposal("services", "Disable telemetry service", true, telemetry),
    proposal("power", "Use High performance plan", false, power, "standard"),
    proposal("services", "Set Xbox save service to manual", false, xbox),
  ],
  alreadyApplied: 3,
  unavailable: 2,
};
const planCalls = () => invoke.mock.calls.filter(([name]) => name === "create_system_change_plan")
  .map(([, bytes]) => JSON.parse(new TextDecoder().decode(bytes as Uint8Array)) as { changes: SystemChange[] });

beforeEach(() => {
  invoke.mockImplementation(async (command: string, bytes?: Uint8Array) => {
    if (command === "get_optimizer_proposals") { expect(bytes).toBeUndefined(); return report; }
    if (command === "system_change_journal") return [];
    if (command === "create_system_change_plan") {
      const { changes } = JSON.parse(new TextDecoder().decode(bytes)) as { changes: SystemChange[] };
      return { planId: "a".repeat(32), requiresHelper: true, expiresInSeconds: 60, changes: changes.map((c) => planned(c, "helper", "planned")) };
    }
    if (command === "confirm_system_change_plan") return;
    if (command === "execute_system_change_plan")
      return { planId: "a".repeat(32), results: [{ change: ads, outcome: { status: "applied" }, journalEntryId: null }] };
    throw new Error(`unexpected ${command}`);
  });
});
afterEach(() => { cleanup(); invoke.mockReset(); });

const box = (name: string) => screen.getByRole("checkbox", { name }) as HTMLInputElement;
const reviewButton = () => screen.getByRole("button", { name: /^Review selected optimizations/ }) as HTMLButtonElement;

it("renders grouped proposals and only the suggested ones start checked, with nothing sent", async () => {
  render(<OptimizerPage />);
  await screen.findByText("Disable telemetry service");
  expect(screen.getByText("3 already applied, 2 unavailable on this device")).toBeTruthy();
  expect(screen.getByText(/Every change is listed separately/)).toBeTruthy();
  expect(box("Disable telemetry service").checked).toBe(true);
  expect(box("Turn off advertising ID").checked).toBe(true);
  expect(box("Set Xbox save service to manual").checked).toBe(false);
  expect(box("Use High performance plan").checked).toBe(false);
  expect(screen.getAllByText(/Needs administrator/)).toHaveLength(2);
  expect(screen.getByText("Effect of Use High performance plan")).toBeTruthy();
  expect(screen.getAllByRole("heading", { level: 2 }).map((h) => h.textContent)).toEqual(
    ["Services", "Privacy", "Power", "Review and apply", "Change history"]);
  expect(reviewButton().textContent).toContain("(2)");
  expect(planCalls()).toHaveLength(0);
});

it("reviews exactly the checked changes in display order and never confirms before Continue", async () => {
  render(<OptimizerPage />);
  await screen.findByText("Disable telemetry service");
  fireEvent.click(box("Use High performance plan"));
  fireEvent.click(reviewButton());
  await screen.findByRole("button", { name: "Continue to Windows confirmation" });
  expect(planCalls()).toEqual([{ changes: [telemetry, ads, power] }]);
  const mutating = () => invoke.mock.calls.filter(([n]) => n === "confirm_system_change_plan" || n === "execute_system_change_plan");
  expect(mutating()).toHaveLength(0);
  fireEvent.click(screen.getByRole("button", { name: "Continue to Windows confirmation" }));
  await waitFor(() => expect(mutating().map(([n]) => n)).toEqual(["confirm_system_change_plan", "execute_system_change_plan"]));
});

it("unchecking a proposal removes it from the request", async () => {
  render(<OptimizerPage />);
  await screen.findByText("Disable telemetry service");
  fireEvent.click(box("Disable telemetry service"));
  fireEvent.click(reviewButton());
  await screen.findByRole("button", { name: "Continue to Windows confirmation" });
  expect(planCalls()).toEqual([{ changes: [ads] }]);
});

it("Clear selection disables review and Select suggested restores the suggested set", async () => {
  render(<OptimizerPage />);
  await screen.findByText("Disable telemetry service");
  fireEvent.click(box("Set Xbox save service to manual"));
  fireEvent.click(screen.getByRole("button", { name: "Clear selection" }));
  expect(screen.getAllByRole("checkbox").some((c) => (c as HTMLInputElement).checked)).toBe(false);
  expect(reviewButton().disabled).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Select suggested" }));
  expect(box("Disable telemetry service").checked).toBe(true);
  expect(box("Set Xbox save service to manual").checked).toBe(false);
  expect(reviewButton().disabled).toBe(false);
  expect(planCalls()).toHaveLength(0);
});

it("offers no one-click apply control", async () => {
  render(<OptimizerPage />);
  await screen.findByText("Disable telemetry service");
  for (const role of ["button", "link", "checkbox", "menuitem"] as const)
    expect(screen.queryAllByRole(role, { name: /apply all|optimize now|one.?click/i })).toHaveLength(0);
});

it("renders in Latin American Spanish", async () => {
  render(<I18nProvider languages={["es-MX"]}><OptimizerPage /></I18nProvider>);
  expect(await screen.findByRole("button", { name: "Seleccionar sugeridas" })).toBeTruthy();
  expect(screen.getByRole("heading", { level: 1, name: "Optimización rápida" })).toBeTruthy();
});
