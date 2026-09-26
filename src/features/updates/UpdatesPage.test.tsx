// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { PlanTicket, SystemChange } from "../../shared/system-change/types";
import { I18nProvider } from "../../shared/i18n/I18nProvider";
import { UpdatesPage } from "./UpdatesPage";
import type { UpdateStatus } from "./types";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const decode = (bytes: unknown) => JSON.parse(new TextDecoder().decode(bytes as Uint8Array)) as { changes: SystemChange[] };
const fixture = (overrides: Partial<UpdateStatus> = {}): UpdateStatus => ({
  apiAvailable: true, serviceEnabled: true, lastSearchSuccess: 1_700_000_000, lastInstallSuccess: null, rebootRequired: false,
  edition: "pro", managed: false, policySupported: true, unsupportedReason: null,
  policies: [
    { id: "au-options", label: "Automatic update behavior", description: "How Windows installs updates.", current: null, applied: false,
      allowed: { kind: "options", options: [{ value: 2, label: "Notify before download" }, { value: 3, label: "Download automatically and notify to install" }] } },
    { id: "defer-feature-updates-days", label: "Defer feature updates (days)", description: "Days to delay.", current: 30, applied: true,
      allowed: { kind: "range", min: 0, max: 365 } },
  ],
  ...overrides,
});
let status = fixture();
const calls = (name: string) => invoke.mock.calls.filter(([command]) => command === name);

beforeEach(() => {
  status = fixture();
  invoke.mockImplementation(async (command: string, bytes?: unknown) => {
    if (command === "get_windows_update_status") return status;
    if (command === "detect_windows_updates") return undefined;
    if (command === "system_change_journal") return [];
    if (command === "create_system_change_plan") {
      const { changes } = decode(bytes);
      return {
        planId: "a".repeat(32), requiresHelper: true, expiresInSeconds: 60,
        changes: changes.map((change) => ({
          change, module: "updates", privilege: "helper", reversibility: { kind: "reversible" },
          impact: { component: "Windows Update policy", effect: "Set value", restart: "none", risk: "medium" },
          expectedPrior: null, inverse: null,
        })),
      } satisfies PlanTicket;
    }
    throw Error(`Forbidden command ${command}`);
  });
});
afterEach(() => { cleanup(); vi.clearAllMocks(); });

it("renders status without selecting or sending anything on load", async () => {
  render(<UpdatesPage />);
  expect(await screen.findByText("Pro")).toBeTruthy();
  expect(screen.getByText("Unknown")).toBeTruthy();
  expect((screen.getByLabelText("Policy value for Automatic update behavior") as HTMLSelectElement).value).toBe("default");
  expect((screen.getByLabelText("Policy value for Defer feature updates (days)") as HTMLInputElement).value).toBe("30");
  expect((screen.getByRole("button", { name: "Review selected changes (0)" }) as HTMLButtonElement).disabled).toBe(true);
  expect(calls("create_system_change_plan")).toHaveLength(0);
  expect(calls("detect_windows_updates")).toHaveLength(0);
});

it("sends exactly the chosen policy value and waits for Windows confirmation", async () => {
  render(<UpdatesPage />);
  const select = await screen.findByLabelText("Policy value for Automatic update behavior");
  fireEvent.change(select, { target: { value: "3" } });
  fireEvent.click(screen.getByRole("button", { name: "Review selected changes (1)" }));
  await screen.findByRole("button", { name: "Continue to Windows confirmation" });
  expect(decode(calls("create_system_change_plan")[0][1])).toEqual({
    changes: [{ kind: "setWindowsUpdatePolicy", settingId: "au-options", value: 3 }],
  });
  expect(calls("confirm_system_change_plan")).toHaveLength(0);
  expect(calls("execute_system_change_plan")).toHaveLength(0);
});

it("maps an emptied number input to the Windows default (null)", async () => {
  render(<UpdatesPage />);
  const input = await screen.findByLabelText("Policy value for Defer feature updates (days)");
  fireEvent.change(input, { target: { value: "" } });
  fireEvent.click(screen.getByRole("button", { name: "Review selected changes (1)" }));
  await screen.findByRole("button", { name: "Continue to Windows confirmation" });
  expect(decode(calls("create_system_change_plan")[0][1])).toEqual({
    changes: [{ kind: "setWindowsUpdatePolicy", settingId: "defer-feature-updates-days", value: null }],
  });
});

it("drops a queued number change when the entry becomes invalid", async () => {
  render(<UpdatesPage />);
  const input = await screen.findByLabelText("Policy value for Defer feature updates (days)");
  fireEvent.change(input, { target: { value: "60" } });
  expect(screen.getByRole("button", { name: "Review selected changes (1)" })).toBeTruthy();
  fireEvent.change(input, { target: { value: "999" } });
  expect(screen.getByRole("alert").textContent).toContain("from 0 to 365");
  expect((screen.getByRole("button", { name: "Review selected changes (0)" }) as HTMLButtonElement).disabled).toBe(true);
});

it("keeps an unlisted current option visible instead of showing Windows default", async () => {
  const base = fixture();
  status = fixture({ policies: [{ ...base.policies[0], current: 7, applied: true }] });
  render(<UpdatesPage />);
  const select = await screen.findByLabelText("Policy value for Automatic update behavior") as HTMLSelectElement;
  expect(select.value).toBe("7");
  expect((screen.getByRole("button", { name: "Review selected changes (0)" }) as HTMLButtonElement).disabled).toBe(true);
});

it("disables every policy control on an unsupported Home edition and explains why", async () => {
  status = fixture({ edition: "home", policySupported: false, unsupportedReason: "editionUnsupported" });
  render(<UpdatesPage />);
  expect(await screen.findByText(/Not honored by this Windows edition/)).toBeTruthy();
  expect((screen.getByLabelText("Policy value for Automatic update behavior") as HTMLSelectElement).disabled).toBe(true);
  expect((screen.getByLabelText("Policy value for Defer feature updates (days)") as HTMLInputElement).disabled).toBe(true);
  expect((screen.getByRole("button", { name: "Review selected changes (0)" }) as HTMLButtonElement).disabled).toBe(true);
  expect(calls("create_system_change_plan")).toHaveLength(0);
});

it("checks for updates on request and then refreshes the status", async () => {
  render(<UpdatesPage />);
  await screen.findByText("Pro");
  expect(calls("get_windows_update_status")).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "Check for updates now" }));
  await waitFor(() => expect(calls("get_windows_update_status")).toHaveLength(2));
  expect(calls("detect_windows_updates")).toHaveLength(1);
  expect(calls("create_system_change_plan")).toHaveLength(0);
});

const startedText = /Windows started checking for updates in the background/;

it("keeps a confirmation visible after Windows starts a background update check", async () => {
  render(<UpdatesPage />);
  await screen.findByText("Pro");
  fireEvent.click(screen.getByRole("button", { name: "Check for updates now" }));
  await waitFor(() => expect(calls("get_windows_update_status")).toHaveLength(2));
  await waitFor(() => expect((screen.getByRole("button", { name: "Check for updates now" }) as HTMLButtonElement).disabled).toBe(false));
  expect(screen.queryByText("Asking Windows to check for updates…")).toBeNull();
  const started = screen.getByText(startedText);
  expect(started.getAttribute("role")).toBe("status");
  expect(started.textContent).toContain("Settings › Windows Update");
  expect(screen.queryByRole("alert")).toBeNull();
});

it("shows the error and no started confirmation when Windows cannot start the check", async () => {
  const base = invoke.getMockImplementation()!;
  invoke.mockImplementation(async (command: string, bytes?: unknown) => {
    if (command === "detect_windows_updates") throw { message: "Update service is not running." };
    return base(command, bytes);
  });
  render(<UpdatesPage />);
  await screen.findByText("Pro");
  fireEvent.click(screen.getByRole("button", { name: "Check for updates now" }));
  expect((await screen.findByRole("alert")).textContent).toBe("Update service is not running.");
  expect(screen.queryByText(startedText)).toBeNull();
  expect(calls("get_windows_update_status")).toHaveLength(1);
});

it("clears an earlier started confirmation when a later check fails", async () => {
  render(<UpdatesPage />);
  await screen.findByText("Pro");
  const button = screen.getByRole("button", { name: "Check for updates now" }) as HTMLButtonElement;
  fireEvent.click(button);
  await screen.findByText(startedText);
  await waitFor(() => expect(button.disabled).toBe(false));
  const base = invoke.getMockImplementation()!;
  invoke.mockImplementation(async (command: string, bytes?: unknown) => {
    if (command === "detect_windows_updates") throw { message: "Update service is not running." };
    return base(command, bytes);
  });
  fireEvent.click(button);
  expect((await screen.findByRole("alert")).textContent).toBe("Update service is not running.");
  expect(screen.queryByText(startedText)).toBeNull();
});

it("renders in Latin American Spanish", async () => {
  render(<I18nProvider languages={["es-MX"]}><UpdatesPage /></I18nProvider>);
  expect(await screen.findByRole("heading", { name: "Directivas de actualización" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Buscar actualizaciones ahora" })).toBeTruthy();
});
