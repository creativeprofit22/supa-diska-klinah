// Minimal IPC fixtures shared by the app-level accessibility and keyboard tests.
// Shapes mirror the per-feature page tests. Unlisted commands reject so error
// states are rendered (and audited) too.

const id = (n: number) => n.toString(16).padStart(32, "0");
const uuid = (n: number) => `0000000${n}-0000-4000-8000-000000000000`;

const firewallRule = (name: string, enabled: boolean) => ({
  name, enabled, direction: "inbound", action: "allow", profiles: 7, applicationName: null, localPorts: "", remoteAddresses: "", grouping: null,
});

const serviceBase = { risk: "low", category: "telemetry", recommended: "disabled", installed: true, delayedAutoStart: false, running: true } as const;

const fixtures: Record<string, unknown> = {
  system_change_journal: [],
  list_drive_inventory: {
    drives: [{ driveId: "opaque-id", displayMount: "C:\\", label: "Windows", filesystem: "NTFS", system: true,
      totalBytes: 1024 ** 4, usedBytes: 1024 ** 3, freeBytes: 1024 ** 4 - 1024 ** 3 }],
    partial: false, warnings: [],
  },
  list_driver_packages: [
    { publishedName: "oem1.inf", originalName: "old.inf", provider: "Contoso", class: "Display", driverDate: "2020-01-01", driverVersion: "1.0", status: "superseded", deletable: true },
    { publishedName: "oem2.inf", originalName: "new.inf", provider: "Contoso", class: "Display", driverDate: "2024-01-01", driverVersion: "2.0", status: "inUse", deletable: false },
  ],
  get_firewall_status: {
    profiles: [
      { profile: "domain", enabled: false, defaultInboundAction: "block", defaultOutboundAction: "allow", blockAllInboundTraffic: false, active: false },
      { profile: "public", enabled: true, defaultInboundAction: "block", defaultOutboundAction: "allow", blockAllInboundTraffic: false, active: true },
    ],
    currentProfiles: ["public"], ruleCount: 2, rulesTruncated: false,
    rules: [firewallRule("Remote Desktop", true), firewallRule("Old Game", false)],
    findings: [
      { id: "rdpOpen", severity: "high", title: "Remote Desktop open", detail: "Inbound RDP is allowed.", relatedRule: "Remote Desktop", relatedProfile: null },
      { id: "domainOff", severity: "medium", title: "Domain profile off", detail: "Firewall is off.", relatedRule: null, relatedProfile: "domain" },
    ],
  },
  get_hosts_report: {
    path: "C:\\Windows\\System32\\drivers\\etc\\hosts", sha256: "b".repeat(64), sizeBytes: 120, totalLines: 3, linesTruncated: false,
    lines: [
      { index: 0, kind: "comment", text: "# Copyright" },
      { index: 1, kind: "mapping", text: "10.0.0.1 update.microsoft.com" },
      { index: 2, kind: "appDisabled", text: "#sdk-disabled# 127.0.0.1 ads.example" },
    ],
    findings: [{ line: 1, severity: "high", kind: "redirect", detail: "update.microsoft.com -> 10.0.0.1" }],
  },
  list_services: [
    { ...serviceBase, id: "diagtrack", serviceName: "DiagTrack", label: "Connected User Experiences", description: "Sends diagnostic data.", start: "automatic" },
    { ...serviceBase, id: "fax", serviceName: "Fax", label: "Fax", description: "Sends faxes.", category: "legacy", installed: false, start: null, running: false },
  ],
  list_startup_items: [
    { name: "Chat", scope: "user", location: "run", source: "run", command: "chat.exe --tray", enabled: true, toggleable: true },
    { name: "OneShot", scope: "user", location: null, source: "runOnce", command: "setup.exe /finish", enabled: true, toggleable: false },
  ],
  get_optimizer_proposals: {
    proposals: [{
      group: "services", label: "Disable telemetry service", suggested: true,
      planned: {
        change: { kind: "setServiceStartType", catalogId: "diagtrack", startType: "disabled" }, module: "services", privilege: "helper",
        reversibility: { kind: "reversible" }, impact: { component: "Service", effect: "Disable", restart: "none", risk: "low" }, expectedPrior: null, inverse: null,
      },
    }],
    alreadyApplied: 3, unavailable: 2,
  },
  get_privacy_report: {
    settings: [
      { id: "advertisingId", hive: "user", label: "Advertising ID", description: "Personalised ads", category: "privacy", current: 1, recommended: 0, applied: false, supported: true, unsupportedReason: null },
      { id: "gameDvr", hive: "machine", label: "Game DVR", description: "Background recording", category: "performance", current: null, recommended: 0, applied: false, supported: false, unsupportedReason: "editionUnsupported" },
    ],
    tasks: [{ id: "ceip", path: "\\Microsoft\\Windows\\CEIP\\Consolidator", label: "CEIP consolidator", enabled: true, recommended: false, present: true }],
    relatedServices: ["DiagTrack"],
  },
  get_power_status: {
    hibernation: { supported: true, enabled: true, hiberfileBytes: 6 * 1024 ** 3 },
    schemes: [
      { id: "381b4222-f694-41f0-9685-ff5bb260df2e", name: "Balanced", active: true },
      { id: "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c", name: "High performance", active: false },
    ],
  },
  list_restore_points: { status: "available", points: [
    { sequenceNumber: 7, description: "Installed Fixture App", createdAt: 1_700_000_000, restorePointType: 0, kind: "applicationInstall" },
  ] },
  get_restore_protection: { policyDisabled: false, protectionEnabled: true, creationFrequencyMinutes: 1440 },
  get_windows_update_status: {
    apiAvailable: true, serviceEnabled: true, lastSearchSuccess: 1_700_000_000, lastInstallSuccess: null, rebootRequired: false,
    edition: "pro", managed: false, policySupported: true, unsupportedReason: null,
    policies: [
      { id: "au-options", label: "Automatic update behavior", description: "How Windows installs updates.", current: null, applied: false,
        allowed: { kind: "options", options: [{ value: 2, label: "Notify before download" }, { value: 3, label: "Download automatically" }] } },
      { id: "defer-feature-updates-days", label: "Defer feature updates (days)", description: "Days to delay.", current: 30, applied: true,
        allowed: { kind: "range", min: 0, max: 365 } },
    ],
  },
  list_scan_schedules: [
    { id: uuid(1), cadence: { kind: "daily", hour: 3, minute: 0 }, enabled: true, lastRun: null, nextRun: "2026-09-24T03:00:00", orphaned: false, orphanReason: null },
    { id: uuid(2), cadence: null, enabled: false, lastRun: null, nextRun: null, orphaned: true, orphanReason: "unrecognizedDefinition" },
  ],
  list_scheduled_scan_summaries: [
    { scheduleId: uuid(1), finishedAt: 1_700_000_000, reclaimableBytes: 2048, itemCount: 4, diagnosticCount: 1, succeeded: true },
  ],
  protection_overview: {
    rules: {
      source: "embeddedBaseline", sequence: 1, created: "2026-09-23T00:00:00Z", description: "Embedded baseline",
      ruleCount: 2, previousSequence: null, recoveryNote: null, externalPacksAllowed: true,
    },
    settings: { network: { ruleDownload: false, passwordBreachCheck: false }, amsiEnabled: false, allowlist: [] },
    quarantineCount: 1, lastScan: null,
    heuristics: [{ id: "H002-double-extension", title: "Double extension", falsePositiveNote: "Some tools do this." }],
  },
  last_protection_scan: {
    scope: "folder", finishedAt: "2026-09-23T00:00:00Z",
    summary: { filesScanned: 3, deterministic: 1, heuristic: 1, unavailable: 1, reparsePointsSkipped: 0, allowlisted: 0, truncated: false, cancelled: false, packSequence: 1 },
    findings: [
      { id: "a".repeat(32), path: "C:\\x\\eicar.com", sha256: "1".repeat(64), size: 68, canQuarantine: true,
        evidence: { kind: "deterministic", ruleId: "eicar.sha256", ruleName: "EICAR anti-malware test file", packSequence: 1, method: "sha256", severity: "low" } },
      { id: "b".repeat(32), path: "C:\\x\\invoice.pdf.exe", sha256: "2".repeat(64), size: 10, canQuarantine: true,
        evidence: { kind: "heuristic", heuristicId: "H002-double-extension", reason: "Name hides the type", falsePositiveNote: "Some tools do this.", severity: "medium" } },
      { id: "c".repeat(32), path: "C:\\x\\locked.bin", sha256: null, size: null, canQuarantine: false, evidence: { kind: "unavailable", reason: "inUse" } },
    ],
  },
  protection_scan_status: { running: false, filesScanned: 0, bytesHashed: 0 },
  list_quarantine: [
    { id: "d".repeat(32), originalPath: "C:\\x\\tool.exe", sha256: "3".repeat(64), size: 5, finding: "Heuristic", quarantinedAt: 1_700_000_000, damaged: false },
  ],
  list_browser_policy: {
    source: "rules/win32/browsers.json", revision: "pinned-revision", lifecycle: "candidate", risk: "highImpact", consequence: "Rebuild caches",
    minimumAgeSeconds: 3600, serviceWorkerDisclosure: "Offline content may be removed", unsupported: ["Unsupported fixture"], exclusions: ["Cookies"],
    profileCacheRoots: ["Cache/Cache_Data"], sharedCacheRoots: ["ShaderCache"],
  },
  list_cleaner_catalog: {
    targets: [{ catalogId: "gpu", targetId: "cache:0", path: "local/Cache", source: "rules/source.ts", revision: "pinned-revision", ruleVersion: 1,
      minimumAgeSeconds: 86400, consequence: "Rebuild cache", exclusions: ["private data"], matcher: "single-file", unsupportedReason: "Unsupported fixture" }],
    unsupportedOperations: [["database", "No database mutation"]],
  },
  vendor_job_history: { records: [], nextCursor: null },
  foundation_status: { platform: "windows", architecture: "x86_64", adapterReady: true },
  preview_cleanup: {
    scanId: "a".repeat(32),
    records: [{ id: "b".repeat(32), ruleId: "temporary-caches", displayPath: "C:\\Users\\fixture\\cache", kind: "directory", bytes: 1024 }],
    diagnostics: [],
  },
  cleanup_history: { records: [], nextCursor: null },
  list_project_roots: [
    { id: "r".repeat(32), displayPath: "C:\\work\\app", paused: false, addedAtUnixSeconds: 1_700_000_000, lastScannedAtUnixSeconds: null },
  ],
  list_build_profiles: [],
  get_active_build_run: null,
  get_scan_settings: { schemaVersion: 1, profile: "auto" },
  get_auto_cleanup_policy: { schemaVersion: 1, enabled: false, graceDays: 7 },
  list_running_programs: {
    processes: [{
      pid: 4242, parentPid: 1, name: "tool.exe", threadCount: 3, imagePath: "C:\\x\\tool.exe", imageLocation: null,
      signer: { state: "unsigned" }, commandLine: { kind: "unavailable", reason: "accessDenied" },
      findings: [{ kind: "heuristic", heuristicId: "H002-double-extension", reason: "Name hides the type", falsePositiveNote: "Some tools do this.", severity: "medium" }],
    }],
    truncated: false, imageUnavailableCount: 0,
  },
};

export const routePaths = [
  "/", "/drives", "/disk-analyzer", "/large-files", "/duplicates", "/empty-folders", "/cleaner", "/browser", "/uninstaller", "/cleanup",
  "/optimizer", "/startup", "/services", "/privacy", "/firewall", "/hosts", "/power", "/drivers", "/restore-points", "/windows-update",
  "/scheduled-scans", "/protection", "/protection/scan", "/protection/processes", "/protection/quarantine", "/protection/rules",
  "/protection/breach", "/settings",
];

export async function respond(command: string, bytes?: unknown): Promise<unknown> {
  if (command === "list_storage_scopes") {
    const input = bytes instanceof Uint8Array || Object.prototype.toString.call(bytes) === "[object Uint8Array]"
      ? JSON.parse(new TextDecoder().decode(bytes as Uint8Array)) as { module?: string } : {};
    return [1, 2].map((n) => ({ scopeId: id(n), module: input.module ?? "cleaner", label: `Scope ${n}`, displayPath: `C:/Fixture${n}`, available: true }));
  }
  if (command in fixtures) return structuredClone(fixtures[command]);
  throw Error(`No fixture for ${command}`);
}
