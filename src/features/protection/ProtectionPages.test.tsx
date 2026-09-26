// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { BreachPage, PASSWORD_TOO_LONG } from "./BreachPage";
import { evidenceKindLabel, summaryLine } from "./labels";
import { OverviewPage } from "./OverviewPage";
import { I18nProvider } from "../../shared/i18n/I18nProvider";
import { COLLISION_HELP, QuarantinePage } from "./QuarantinePage";
import { RulesPage } from "./RulesPage";
import { ScanPage } from "./ScanPage";
import type { ProtectionOverview, QuarantineEntry, ScanReport, ScanStatus } from "./types";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const overview = (): ProtectionOverview => ({
  rules: {
    source: "embeddedBaseline", sequence: 1, created: "2026-09-23T00:00:00Z", description: "Embedded baseline",
    ruleCount: 2, previousSequence: null, recoveryNote: null, externalPacksAllowed: true,
  },
  settings: { network: { ruleDownload: false, passwordBreachCheck: false }, amsiEnabled: false, allowlist: [] },
  quarantineCount: 0,
  lastScan: null,
  heuristics: [{ id: "H002-double-extension", title: "Double extension", falsePositiveNote: "Some tools do this." }],
});

const report: ScanReport = {
  scope: "folder",
  finishedAt: "2026-09-23T00:00:00Z",
  summary: { filesScanned: 3, deterministic: 1, heuristic: 1, unavailable: 1, reparsePointsSkipped: 0, allowlisted: 0, truncated: false, cancelled: false, packSequence: 1 },
  findings: [
    { id: "a".repeat(32), path: "C:\\x\\eicar.com", sha256: "1".repeat(64), size: 68, canQuarantine: true,
      evidence: { kind: "deterministic", ruleId: "eicar.sha256", ruleName: "EICAR anti-malware test file", packSequence: 1, method: "sha256", severity: "low" } },
    { id: "b".repeat(32), path: "C:\\x\\invoice.pdf.exe", sha256: "2".repeat(64), size: 10, canQuarantine: true,
      evidence: { kind: "heuristic", heuristicId: "H002-double-extension", reason: "Name hides the type", falsePositiveNote: "Some tools do this.", severity: "medium" } },
    { id: "c".repeat(32), path: "C:\\x\\locked.bin", sha256: null, size: null, canQuarantine: false,
      evidence: { kind: "unavailable", reason: "inUse" } },
  ],
};

const entry: QuarantineEntry = { id: "d".repeat(32), originalPath: "C:\\x\\tool.exe", sha256: "3".repeat(64), size: 5, finding: "Heuristic", quarantinedAt: 1_700_000_000, damaged: false };

const decode = (bytes?: Uint8Array) => (bytes ? JSON.parse(new TextDecoder().decode(bytes)) : undefined);
const callsTo = (name: string) => invoke.mock.calls.filter(([command]) => command === name).map(([, bytes]) => decode(bytes));
let state: ProtectionOverview;
let status: ScanStatus;

beforeEach(() => {
  state = overview();
  status = { running: false, filesScanned: 0, bytesHashed: 0 };
  invoke.mockImplementation(async (command: string, bytes?: Uint8Array) => {
    // Raw JSON object body (realm-safe check; jsdom and Node have separate Uint8Array constructors).
    expect(Object.prototype.toString.call(bytes)).toBe("[object Uint8Array]");
    expect(bytes![0]).toBe("{".charCodeAt(0));
    switch (command) {
      case "protection_overview": return state;
      case "set_protection_network_policy": {
        const input = decode(bytes);
        state = { ...state, settings: { ...state.settings, network: input.network, amsiEnabled: input.amsiEnabled } };
        return state.settings;
      }
      case "last_protection_scan": return report;
      case "protection_scan_status": return status;
      case "cancel_protection_scan": return null;
      case "list_quarantine": return [entry];
      default: throw Error(`Unexpected command ${command}`);
    }
  });
});
afterEach(() => { cleanup(); vi.clearAllMocks(); });

const inRouter = (ui: React.ReactNode) => render(<MemoryRouter>{ui}</MemoryRouter>);

it("defaults every network feature to off and makes no network command on load", async () => {
  inRouter(<OverviewPage />);
  const boxes = await screen.findAllByRole("checkbox");
  expect(boxes).toHaveLength(3);
  expect(boxes.every((box) => !(box as HTMLInputElement).checked)).toBe(true);
  expect(callsTo("download_rule_pack")).toHaveLength(0);
  expect(callsTo("check_password_breach")).toHaveLength(0);
  expect(screen.getByText(/Built-in baseline pack/)).toBeTruthy();
});

it("each toggle changes only its own flag", async () => {
  inRouter(<OverviewPage />);
  fireEvent.click(await screen.findByLabelText("Allow the password breach check"));
  await waitFor(() => expect(callsTo("set_protection_network_policy")).toHaveLength(1));
  expect(callsTo("set_protection_network_policy")[0]).toEqual({ network: { ruleDownload: false, passwordBreachCheck: true }, amsiEnabled: false });
});

it("keeps keyboard focus on a toggle while its setting saves", async () => {
  let release: () => void = () => undefined;
  inRouter(<OverviewPage />);
  const toggle = await screen.findByLabelText("Allow downloading signed rule packs") as HTMLInputElement;
  const saved = invoke.getMockImplementation();
  invoke.mockImplementation(async (command: string, bytes?: Uint8Array) => {
    if (command === "set_protection_network_policy") await new Promise<void>((resolve) => { release = resolve; });
    return saved?.(command, bytes);
  });
  toggle.focus();
  fireEvent.click(toggle);
  await waitFor(() => expect(callsTo("set_protection_network_policy")).toHaveLength(1));
  expect(toggle.disabled).toBe(false);
  expect(document.activeElement).toBe(toggle);
  fireEvent.click(toggle); // ignored while the first save is running
  expect(callsTo("set_protection_network_policy")).toHaveLength(1);
  release();
  await waitFor(() => expect(toggle.checked).toBe(true));
  expect(document.activeElement).toBe(toggle);
});

it("groups results by evidence type and never claims a clean result", async () => {
  const { container } = inRouter(<ScanPage />);
  await screen.findByText(/C:\\x\\eicar.com/);
  for (const kind of ["deterministic", "heuristic", "unavailable"] as const) {
    expect(screen.getByRole("heading", { name: new RegExp(`^${evidenceKindLabel[kind]}`) })).toBeTruthy();
  }
  expect(screen.getByText("In use by another program")).toBeTruthy();
  expect(screen.getByText(/False positives: Some tools do this/)).toBeTruthy();
  // Only heuristics can be hidden; unavailable items cannot be quarantined.
  expect(screen.getAllByRole("button", { name: "Hide for this file" })).toHaveLength(1);
  expect(screen.getAllByRole("button", { name: "Quarantine…" })).toHaveLength(2);
  expect(container.textContent).not.toMatch(/\b(clean|safe|certified|protected)\b/i);
});

it("reattaches to a scan still running in the backend", async () => {
  status = { running: true, filesScanned: 42, bytesHashed: 1024 };
  const { container } = inRouter(<ScanPage />);
  expect(await screen.findByText("Scanning… 42 files examined")).toBeTruthy();
  expect((screen.getByRole("button", { name: "Quick scan" }) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Cancel scan" }));
  await waitFor(() => expect(callsTo("cancel_protection_scan")).toHaveLength(1));
  expect(container.textContent).not.toMatch(/\b(clean|safe|certified|protected)\b/i);
});

it("reports skipped links and junctions in the scan summary", async () => {
  const saved = invoke.getMockImplementation();
  invoke.mockImplementation(async (command: string, bytes?: Uint8Array) => command === "last_protection_scan"
    ? { ...report, summary: { ...report.summary, reparsePointsSkipped: 3 } }
    : saved?.(command, bytes));
  const { container } = inRouter(<ScanPage />);
  expect(await screen.findByText(/3 links or junctions skipped \(not followed\)/)).toBeTruthy();
  expect(container.textContent).not.toMatch(/\b(clean|safe|certified|protected)\b/i);
});

it("omits the skipped-links note when none were skipped", async () => {
  inRouter(<ScanPage />);
  await screen.findByText(/C:\\x\\eicar.com/);
  expect(screen.queryByText(/links or junctions skipped/)).toBeNull();
});

it("says when Defender history was cut off", async () => {
  const saved = invoke.getMockImplementation();
  invoke.mockImplementation(async (command: string, bytes?: Uint8Array) => command === "get_defender_history"
    ? { detections: [{ threatId: "2147519003", detectedAt: "2026-09-01T00:00:00Z", resources: ["C:\\x\\eicar.com"], evidence: { kind: "external", provider: "Microsoft Defender", observedAt: "2026-09-01T00:00:00Z", detail: "EICAR" } }], unavailable: null, truncated: true }
    : saved?.(command, bytes));
  inRouter(<OverviewPage />);
  fireEvent.click(await screen.findByRole("button", { name: "Read detection history" }));
  expect(await screen.findByText("Showing the first 500 recorded detections.")).toBeTruthy();
});

it("summary wording reports no matches without implying safety", () => {
  const line = summaryLine({ filesScanned: 10, deterministic: 0, heuristic: 0, unavailable: 2, packSequence: 4 });
  expect(line).toContain("no signed-rule matches");
  expect(line).toContain("rule pack 4");
  expect(line).toContain("2 items not checked");
  expect(line).not.toMatch(/clean|safe/i);
});

it("restore collision explains that nothing was overwritten", async () => {
  invoke.mockImplementationOnce(async () => [entry]).mockImplementationOnce(async () => {
    throw { code: { quarantine: "collision" }, message: "a file already exists" };
  });
  inRouter(<QuarantinePage />);
  fireEvent.click(await screen.findByRole("button", { name: "Restore…" }));
  expect(await screen.findByRole("alert")).toHaveProperty("textContent", COLLISION_HELP);
  expect(callsTo("restore_quarantined")).toEqual([{ id: entry.id }]);
});

it("a declined native confirmation shows no error", async () => {
  invoke.mockImplementationOnce(async () => [entry]).mockImplementationOnce(async () => {
    throw { code: "confirmationDeclined", message: "cancelled" };
  });
  inRouter(<QuarantinePage />);
  fireEvent.click(await screen.findByRole("button", { name: "Delete permanently…" }));
  await waitFor(() => expect(callsTo("delete_quarantined")).toHaveLength(1));
  expect(screen.queryByRole("alert")).toBeNull();
});

it("download stays disabled until its opt-in is on", async () => {
  inRouter(<RulesPage />);
  expect((await screen.findByRole("button", { name: "Download latest pack" }) as HTMLButtonElement).disabled).toBe(true);
  expect(screen.getByText(/Downloading is off/)).toBeTruthy();
});

it("password check is disabled while its opt-in is off", async () => {
  inRouter(<BreachPage />);
  expect(await screen.findByText(/This check is off/)).toBeTruthy();
  expect((screen.getByLabelText("Password") as HTMLInputElement).disabled).toBe(true);
  expect(callsTo("check_password_breach")).toHaveLength(0);
});

it.each([
  ["1025 ASCII characters", "a".repeat(1025)],
  ["513 two-byte characters (1026 bytes)", "é".repeat(513)],
])("rejects a password over 1024 bytes without calling the backend: %s", async (_label, value) => {
  state = { ...overview(), settings: { ...overview().settings, network: { ruleDownload: false, passwordBreachCheck: true } } };
  inRouter(<BreachPage />);
  const input = screen.getByLabelText("Password") as HTMLInputElement;
  await waitFor(() => expect(input.disabled).toBe(false));
  expect(input.maxLength).toBe(1024);
  fireEvent.change(input, { target: { value } });
  fireEvent.click(screen.getByRole("button", { name: "Check" }));
  expect((await screen.findByRole("alert")).textContent).toBe("Passwords longer than 1024 bytes can't be checked.");
  expect(PASSWORD_TOO_LONG).toBe("Passwords longer than 1024 bytes can't be checked.");
  expect(callsTo("check_password_breach")).toHaveLength(0);
});
it("renders Spanish (es-419) overview and scan text when the language is Spanish", async () => {
  inRouter(<I18nProvider languages={["es-MX"]}><OverviewPage /></I18nProvider>);
  expect(await screen.findByRole("heading", { name: "Uso de red" })).toBeTruthy();
  expect(screen.getByText(/Paquete base integrado: secuencia 1, 2 reglas/)).toBeTruthy();
  cleanup();
  inRouter(<I18nProvider languages={["es-MX"]}><ScanPage /></I18nProvider>);
  expect(await screen.findByText(/3 archivos examinados con el paquete de reglas 1/)).toBeTruthy();
  expect(screen.getAllByText("Coincidencia con regla firmada").length).toBeGreaterThan(0);
});
