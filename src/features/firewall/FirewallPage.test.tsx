// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { PlanTicket, SystemChange } from "../../shared/system-change/types";
import { I18nProvider } from "../../shared/i18n/I18nProvider";
import { FirewallPage } from "./FirewallPage";
import type { FirewallRule, FirewallStatus } from "./types";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const rule = (name: string, enabled: boolean): FirewallRule => ({
  name, enabled, direction: "inbound", action: "allow", profiles: 7, applicationName: null, localPorts: "", remoteAddresses: "", grouping: null,
});
const fixture: FirewallStatus = {
  profiles: [
    { profile: "domain", enabled: false, defaultInboundAction: "block", defaultOutboundAction: "allow", blockAllInboundTraffic: false, active: false },
    { profile: "public", enabled: true, defaultInboundAction: "block", defaultOutboundAction: "allow", blockAllInboundTraffic: false, active: true },
  ],
  currentProfiles: ["public"],
  ruleCount: 252,
  rulesTruncated: false,
  rules: [
    rule("Remote Desktop", true),
    rule("Old Game", false),
    ...Array.from({ length: 250 }, (_, i) => rule(`Filler ${i}`, true)),
  ],
  findings: [
    { id: "rdpOpen", severity: "high", title: "Remote Desktop open", detail: "Inbound RDP is allowed.", relatedRule: "Remote Desktop", relatedProfile: null },
    { id: "stale", severity: "low", title: "Old game rule", detail: "Already disabled.", relatedRule: "Old Game", relatedProfile: null },
    { id: "domainOff", severity: "medium", title: "Domain profile off", detail: "Firewall is off.", relatedRule: null, relatedProfile: "domain" },
  ],
};

const decode = (bytes: Uint8Array) => JSON.parse(new TextDecoder().decode(bytes));
const planned = () => invoke.mock.calls.filter(([name]) => name === "create_system_change_plan").map(([, bytes]) => decode(bytes).changes as SystemChange[]);
const called = (name: string) => invoke.mock.calls.some(([command]) => command === name);

beforeEach(() => {
  invoke.mockImplementation(async (command: string, bytes?: Uint8Array) => {
    if (command === "get_firewall_status") { expect(bytes).toBeUndefined(); return fixture; }
    if (command === "system_change_journal") return [];
    if (command === "create_system_change_plan") {
      const { changes } = decode(bytes!) as { changes: SystemChange[] };
      return {
        planId: "a".repeat(32), requiresHelper: true, expiresInSeconds: 60,
        changes: changes.map((change) => ({
          change, module: "firewall", privilege: "helper", reversibility: { kind: "reversible" },
          impact: { component: "Windows Firewall", effect: "Changes firewall state", restart: "none", risk: "high" }, expectedPrior: null, inverse: null,
        })),
      } satisfies PlanTicket;
    }
    throw Error(`Unexpected command ${command}`);
  });
});
afterEach(() => { cleanup(); vi.clearAllMocks(); });

const review = () => fireEvent.click(screen.getByRole("button", { name: "Review selected changes (1)" }));

it("renders the status with nothing selected or sent on load", async () => {
  render(<FirewallPage />);
  expect(await screen.findByText("Remote Desktop open")).toBeTruthy();
  expect(screen.getAllByRole("checkbox").every((box) => !(box as HTMLInputElement).checked)).toBe(true);
  expect((screen.getByRole("button", { name: "Review selected changes (0)" }) as HTMLButtonElement).disabled).toBe(true);
  expect(called("create_system_change_plan")).toBe(false);
});

it("a finding checkbox sends exactly the rule change, and nothing is confirmed before Windows confirmation", async () => {
  render(<FirewallPage />);
  fireEvent.click(await screen.findByLabelText("Turn off rule Remote Desktop", { selector: "li input" }));
  // The matching rule row reflects the same, deduplicated change.
  expect((screen.getByLabelText("Turn off rule Remote Desktop", { selector: "tr input" }) as HTMLInputElement).checked).toBe(true);
  review();
  await screen.findByRole("button", { name: "Continue to Windows confirmation" });
  expect(planned()).toEqual([[{ kind: "setFirewallRuleEnabled", ruleName: "Remote Desktop", enabled: false }]]);
  expect(called("confirm_system_change_plan") || called("execute_system_change_plan")).toBe(false);
});

it("deduplicates a finding and its rule row into one change", async () => {
  render(<FirewallPage />);
  fireEvent.click(await screen.findByLabelText("Turn off rule Remote Desktop", { selector: "tr input" }));
  expect((screen.getByLabelText("Turn off rule Remote Desktop", { selector: "li input" }) as HTMLInputElement).checked).toBe(true);
  expect(screen.getByRole("button", { name: "Review selected changes (1)" })).toBeTruthy();
});

it("a profile toggle sends the exact change and turning one off shows the high-risk warning", async () => {
  render(<FirewallPage />);
  expect(await screen.findByLabelText("Change Domain to on")).toBeTruthy();
  expect(screen.queryByText(/leaves this PC open/)).toBeNull();
  fireEvent.click(screen.getByLabelText("Change Public to off"));
  expect(screen.getByText(/leaves this PC open/).textContent).toContain("Public");
  review();
  await screen.findByRole("button", { name: "Continue to Windows confirmation" });
  expect(planned()).toEqual([[{ kind: "setFirewallProfileEnabled", profile: "public", enabled: false }]]);
});

it("findings for rules that are already off cannot be selected", async () => {
  render(<FirewallPage />);
  const box = (await screen.findByLabelText("Turn off rule Old Game")) as HTMLInputElement;
  expect(box.disabled).toBe(true);
  expect(screen.getByText(/Rule is already off/)).toBeTruthy();
  fireEvent.click(box);
  expect(screen.getByRole("button", { name: "Review selected changes (0)" })).toBeTruthy();
});

it("the filter limits rule rows to the first 200 matches", async () => {
  render(<FirewallPage />);
  await screen.findByText("Remote Desktop open");
  const rows = () => screen.getAllByRole("row").length - 1;
  expect(rows()).toBe(200);
  fireEvent.change(screen.getByLabelText("Filter rules"), { target: { value: "old game" } });
  expect(rows()).toBe(1);
  fireEvent.click(screen.getByLabelText("Turn on rule Old Game"));
  review();
  await screen.findByRole("button", { name: "Continue to Windows confirmation" });
  expect(planned()).toEqual([[{ kind: "setFirewallRuleEnabled", ruleName: "Old Game", enabled: true }]]);
});

it("renders in Latin American Spanish", async () => {
  render(<I18nProvider languages={["es-MX"]}><FirewallPage /></I18nProvider>);
  expect(await screen.findByRole("heading", { name: "Perfiles" })).toBeTruthy();
  expect(screen.getByRole("heading", { name: "Reglas" })).toBeTruthy();
});
