// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { PlanTicket, SystemChange } from "../../shared/system-change/types";
import { I18nProvider } from "../../shared/i18n/I18nProvider";
import { PrivacyPage } from "./PrivacyPage";
import type { PrivacyReport } from "./types";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const fixture: PrivacyReport = {
  settings: [
    { id: "advertisingId", hive: "user", label: "Advertising ID", description: "Personalised ads", category: "privacy", current: 1, recommended: 0, applied: false, supported: true, unsupportedReason: null },
    { id: "allowTelemetry", hive: "machine", label: "Diagnostic data", description: "Telemetry level", category: "privacy", current: null, recommended: 0, applied: false, supported: true, unsupportedReason: null },
    { id: "tailoredExperiences", hive: "user", label: "Tailored experiences", description: "Tips", category: "privacy", current: 0, recommended: 0, applied: true, supported: true, unsupportedReason: null },
    { id: "gameDvr", hive: "machine", label: "Game DVR", description: "Background recording", category: "performance", current: null, recommended: 0, applied: false, supported: false, unsupportedReason: "editionUnsupported" },
  ],
  tasks: [
    { id: "compatAppraiser", path: "\Microsoft\Windows\Application Experience\Compat", label: "Compatibility appraiser", enabled: true, recommended: false, present: true },
    { id: "ceip", path: "\Microsoft\Windows\CEIP\Consolidator", label: "CEIP consolidator", enabled: false, recommended: false, present: true },
  ],
  relatedServices: ["DiagTrack"],
};

const calls = (name: string) => invoke.mock.calls.filter(([command]) => command === name);
const sent = () => calls("create_system_change_plan").map(([, bytes]) => (JSON.parse(new TextDecoder().decode(bytes as Uint8Array)) as { changes: SystemChange[] }).changes);

beforeEach(() => {
  invoke.mockImplementation(async (command: string, bytes?: Uint8Array) => {
    if (command === "get_privacy_report") { expect(bytes).toBeUndefined(); return fixture; }
    if (command === "system_change_journal") return [];
    if (command === "create_system_change_plan") {
      const { changes } = JSON.parse(new TextDecoder().decode(bytes)) as { changes: SystemChange[] };
      return {
        planId: "a".repeat(32), requiresHelper: true, expiresInSeconds: 60,
        changes: changes.map((change) => ({ change, module: "privacy", privilege: "helper", reversibility: { kind: "reversible" }, impact: { component: "Privacy", effect: "Change", restart: "none", risk: "low" }, expectedPrior: null, inverse: null })),
      } satisfies PlanTicket;
    }
    throw Error(`Unexpected command ${command}`);
  });
});
afterEach(() => { cleanup(); vi.clearAllMocks(); });

async function loaded() {
  render(<PrivacyPage />);
  await screen.findByText("Advertising ID");
}

async function review(label: string) {
  fireEvent.click(screen.getByRole("checkbox", { name: label }));
  fireEvent.click(screen.getByRole("button", { name: "Review selected changes (1)" }));
  await screen.findByRole("button", { name: "Continue to Windows confirmation" });
  return sent();
}

it("renders grouped settings with nothing selected or sent on load", async () => {
  await loaded();
  expect(screen.getByRole("heading", { name: "Privacy", level: 2 })).toBeTruthy();
  expect(screen.getByRole("heading", { name: "Performance" })).toBeTruthy();
  expect(screen.getByText(/DiagTrack.*Services page/)).toBeTruthy();
  for (const box of screen.getAllByRole("checkbox")) expect((box as HTMLInputElement).checked).toBe(false);
  expect((screen.getByRole("button", { name: "Review selected changes (0)" }) as HTMLButtonElement).disabled).toBe(true);
  expect(calls("create_system_change_plan")).toHaveLength(0);
});

it("sends setUserSetting for a user-hive setting and does not confirm before the Windows step", async () => {
  await loaded();
  expect(await review("Apply recommended for Advertising ID")).toEqual([[{ kind: "setUserSetting", settingId: "advertisingId", value: 0 }]]);
  expect(calls("confirm_system_change_plan")).toHaveLength(0);
  expect(calls("execute_system_change_plan")).toHaveLength(0);
});

it("sends setMachineSetting for a machine-hive setting", async () => {
  await loaded();
  expect(await review("Apply recommended for Diagnostic data")).toEqual([[{ kind: "setMachineSetting", settingId: "allowTelemetry", value: 0 }]]);
});

it("restores the Windows default with value null for an applied setting", async () => {
  await loaded();
  expect(screen.queryByRole("checkbox", { name: "Apply recommended for Tailored experiences" })).toBeNull();
  expect(await review("Restore Windows default for Tailored experiences")).toEqual([[{ kind: "setUserSetting", settingId: "tailoredExperiences", value: null }]]);
});

it("shows the reason and no checkbox for an unsupported setting", async () => {
  await loaded();
  expect(screen.getByText(/Not honored by this Windows edition/)).toBeTruthy();
  expect(screen.queryByRole("checkbox", { name: /Game DVR/ })).toBeNull();
});

it("offers only tasks whose state differs from the recommendation", async () => {
  await loaded();
  expect(screen.queryByRole("checkbox", { name: /CEIP consolidator/ })).toBeNull();
  expect(await review("Apply recommended for Compatibility appraiser")).toEqual([[{ kind: "setSystemTaskEnabled", catalogId: "compatAppraiser", enabled: false }]]);
  await waitFor(() => expect(calls("create_system_change_plan")).toHaveLength(1));
});

it("renders in Latin American Spanish", async () => {
  render(<I18nProvider languages={["es-MX"]}><PrivacyPage /></I18nProvider>);
  expect(await screen.findByRole("heading", { name: "Tareas programadas" })).toBeTruthy();
  expect(screen.getByRole("heading", { level: 1, name: "Privacidad" })).toBeTruthy();
});
