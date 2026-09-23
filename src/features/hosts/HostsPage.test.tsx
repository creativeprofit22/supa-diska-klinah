// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { PlanTicket, SystemChange } from "../../shared/system-change/types";
import { HostsPage } from "./HostsPage";
import type { HostsReport } from "./types";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const fixture: HostsReport = {
  path: "C:\\Windows\\System32\\drivers\\etc\\hosts",
  sha256: "b".repeat(64),
  sizeBytes: 120,
  totalLines: 5,
  linesTruncated: false,
  lines: [
    { index: 0, kind: "comment", text: "# Copyright" },
    { index: 1, kind: "blank", text: "" },
    { index: 2, kind: "mapping", text: "10.0.0.1 update.microsoft.com" },
    { index: 3, kind: "appDisabled", text: "#sdk-disabled# 127.0.0.1 ads.example" },
    { index: 4, kind: "invalid", text: "garbage" },
  ],
  findings: [{ line: 2, severity: "high", kind: "redirect", detail: "update.microsoft.com -> 10.0.0.1" }],
};

const decode = (bytes: Uint8Array) => JSON.parse(new TextDecoder().decode(bytes));
const planned = () => invoke.mock.calls.filter(([name]) => name === "create_system_change_plan").map(([, bytes]) => decode(bytes).changes as SystemChange[]);
const called = (name: string) => invoke.mock.calls.some(([command]) => command === name);

beforeEach(() => {
  invoke.mockImplementation(async (command: string, bytes?: Uint8Array) => {
    if (command === "get_hosts_report") { expect(bytes).toBeUndefined(); return fixture; }
    if (command === "system_change_journal") return [];
    if (command === "create_system_change_plan") {
      const { changes } = decode(bytes!) as { changes: SystemChange[] };
      return {
        planId: "a".repeat(32), requiresHelper: true, expiresInSeconds: 60,
        changes: changes.map((change) => ({
          change, module: "hosts", privilege: "helper", reversibility: { kind: "reversibleWithBackup" },
          impact: { component: "Hosts file", effect: "Edits hosts lines", restart: "none", risk: "medium" }, expectedPrior: null, inverse: null,
        })),
      } satisfies PlanTicket;
    }
    throw Error(`Unexpected command ${command}`);
  });
});
afterEach(() => { cleanup(); vi.clearAllMocks(); });

it("renders findings and lines with nothing selected or sent on load, and explains the backup", async () => {
  render(<HostsPage />);
  expect(await screen.findByText(/update.microsoft.com -> 10.0.0.1/)).toBeTruthy();
  expect(screen.getByText(/a backup of the hosts file is made/)).toBeTruthy();
  expect(screen.getByText(/only changed if it is still exactly as it was/)).toBeTruthy();
  expect(screen.getAllByRole("checkbox").every((box) => !(box as HTMLInputElement).checked)).toBe(true);
  expect(called("create_system_change_plan")).toBe(false);
});

it("comment, blank and invalid lines have no checkbox", async () => {
  render(<HostsPage />);
  await screen.findByLabelText("Disable line 3");
  expect(screen.getAllByRole("checkbox").map((box) => box.parentElement?.textContent)).toEqual(["Disable line 3", "Restore line 4"]);
  expect(screen.queryByLabelText(/line 1$/)).toBeNull();
  expect(screen.queryByLabelText(/line 2$/)).toBeNull();
  expect(screen.queryByLabelText(/line 5$/)).toBeNull();
});

it("one selected line sends exactly one editHosts change; nothing is confirmed early", async () => {
  render(<HostsPage />);
  fireEvent.click(await screen.findByLabelText("Disable line 3"));
  fireEvent.click(screen.getByRole("button", { name: "Review selected changes (1)" }));
  await screen.findByRole("button", { name: "Continue to Windows confirmation" });
  expect(planned()).toEqual([[{ kind: "editHosts", lineOps: [{ line: 2, action: "disable" }] }]]);
  expect(called("confirm_system_change_plan") || called("execute_system_change_plan")).toBe(false);
});

it("two selected lines produce one editHosts change with two ops in line order", async () => {
  render(<HostsPage />);
  fireEvent.click(await screen.findByLabelText("Restore line 4"));
  fireEvent.click(screen.getByLabelText("Disable line 3"));
  fireEvent.click(screen.getByRole("button", { name: "Review selected changes (1)" }));
  await screen.findByRole("button", { name: "Continue to Windows confirmation" });
  expect(planned()).toEqual([[{ kind: "editHosts", lineOps: [{ line: 2, action: "disable" }, { line: 3, action: "restore" }] }]]);
});
