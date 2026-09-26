// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { PlanTicket, SystemChange } from "../../shared/system-change/types";
import { I18nProvider } from "../../shared/i18n/I18nProvider";
import { DriversPage } from "./DriversPage";
import type { DriverPackage } from "./types";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const fixture: DriverPackage[] = [
  { publishedName: "oem1.inf", originalName: "old.inf", provider: "Contoso", class: "Display", driverDate: "2020-01-01", driverVersion: "1.0", status: "superseded", deletable: true },
  { publishedName: "oem2.inf", originalName: "new.inf", provider: "Contoso", class: "Display", driverDate: "2024-01-01", driverVersion: "2.0", status: "inUse", deletable: false },
  { publishedName: "oem3.inf", originalName: "solo.inf", provider: "Fabrikam", class: "Net", driverDate: null, driverVersion: null, status: "current", deletable: false },
];
const decode = (bytes: unknown) => JSON.parse(new TextDecoder().decode(bytes as Uint8Array)) as { changes: SystemChange[] };
const called = (name: string) => invoke.mock.calls.filter(([command]) => command === name);

beforeEach(() => {
  invoke.mockImplementation(async (command: string, bytes?: Uint8Array) => {
    if (command === "list_driver_packages") return fixture;
    if (command === "system_change_journal") return [];
    if (command === "create_system_change_plan") {
      const { changes } = decode(bytes);
      return {
        planId: "a".repeat(32), requiresHelper: true, expiresInSeconds: 60,
        changes: changes.map((change) => ({
          change, module: "drivers", privilege: "helper", expectedPrior: null, inverse: null,
          reversibility: { kind: "irreversible", reason: "driver store" },
          impact: { component: "Driver store", effect: "Removes a package", restart: "none", risk: "medium" },
        })),
      } satisfies PlanTicket;
    }
    throw Error(`Unexpected command ${command}`);
  });
});
afterEach(() => { cleanup(); vi.clearAllMocks(); });

async function selectFirst() {
  render(<DriversPage />);
  const box = await screen.findByRole("checkbox", { name: /oem1\.inf/ });
  expect((box as HTMLInputElement).checked).toBe(false);
  expect(called("create_system_change_plan")).toHaveLength(0);
  fireEvent.click(box);
}

it("renders packages with status, nothing selected or sent on load, and no checkbox for non-deletable packages", async () => {
  render(<DriversPage />);
  await screen.findByText(/oem2\.inf/);
  expect(screen.getByText(/In use by a present device/)).toBeTruthy();
  expect(screen.getByText(/Superseded by a newer package/)).toBeTruthy();
  expect(screen.getByText(/cannot be undone/)).toBeTruthy();
  expect(screen.getByText(/Driver update installation is not offered/)).toBeTruthy();
  expect(screen.queryByRole("checkbox", { name: /oem2\.inf/ })).toBeNull();
  expect(screen.queryByRole("checkbox", { name: /oem3\.inf/ })).toBeNull();
  expect(screen.getByText("Cannot be removed: a present device uses it.")).toBeTruthy();
  const restore = screen.getByRole("checkbox", { name: "Create a restore point first" }) as HTMLInputElement;
  expect(restore.checked).toBe(false);
  expect(restore.disabled).toBe(true);
  expect((screen.getByRole("button", { name: "Review selected changes (0)" }) as HTMLButtonElement).disabled).toBe(true);
  expect(called("create_system_change_plan")).toHaveLength(0);
});

it("sends restore point then delete, and does not confirm or execute before Windows confirmation", async () => {
  await selectFirst();
  expect((screen.getByRole("checkbox", { name: "Create a restore point first" }) as HTMLInputElement).checked).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Review selected changes (2)" }));
  await screen.findByRole("button", { name: "Continue to Windows confirmation" });
  const calls = called("create_system_change_plan");
  expect(calls).toHaveLength(1);
  expect(decode(calls[0][1])).toEqual({ changes: [
    { kind: "createRestorePoint", description: "Before removing driver packages" },
    { kind: "deleteDriverPackage", publishedName: "oem1.inf" },
  ] });
  expect(called("confirm_system_change_plan")).toHaveLength(0);
  expect(called("execute_system_change_plan")).toHaveLength(0);
});

it("sends only the delete when the restore point is unchecked", async () => {
  await selectFirst();
  fireEvent.click(screen.getByRole("checkbox", { name: "Create a restore point first" }));
  fireEvent.click(screen.getByRole("button", { name: "Review selected changes (1)" }));
  await screen.findByRole("button", { name: "Continue to Windows confirmation" });
  expect(decode(called("create_system_change_plan")[0][1])).toEqual({ changes: [{ kind: "deleteDriverPackage", publishedName: "oem1.inf" }] });
  expect(called("confirm_system_change_plan")).toHaveLength(0);
  expect(called("execute_system_change_plan")).toHaveLength(0);
});

it("re-checks the restore point by default after the selection is emptied and made again", async () => {
  await selectFirst();
  const restore = () => screen.getByRole("checkbox", { name: "Create a restore point first" }) as HTMLInputElement;
  fireEvent.click(restore());
  expect(restore().checked).toBe(false);
  const box = screen.getByRole("checkbox", { name: /oem1\.inf/ });
  fireEvent.click(box);
  expect(restore().checked).toBe(false);
  fireEvent.click(box);
  expect(restore().checked).toBe(true);
  expect(screen.getByRole("button", { name: "Review selected changes (2)" })).toBeTruthy();
});

it("renders in Latin American Spanish", async () => {
  render(<I18nProvider languages={["es-MX"]}><DriversPage /></I18nProvider>);
  expect(await screen.findByRole("checkbox", { name: "Crear primero un punto de restauración" })).toBeTruthy();
  expect(screen.getByRole("heading", { level: 1, name: "Paquetes de controladores" })).toBeTruthy();
});
