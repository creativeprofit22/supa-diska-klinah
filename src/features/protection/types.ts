export type Severity = "low" | "medium" | "high";
export type MatchMethod = "sha256" | "bytes" | "fileName";
export type UnavailableReason =
  | "accessDenied" | "inUse" | "tooLarge" | "reparsePoint" | "notFound"
  | "signerNotCheckable" | "providerAbsent" | "providerNoResponse" | "cancelled" | "readFailed";

export type Evidence =
  | { kind: "deterministic"; ruleId: string; ruleName: string; packSequence: number; method: MatchMethod; severity: Severity }
  | { kind: "heuristic"; heuristicId: string; reason: string; falsePositiveNote: string; severity: Severity }
  | { kind: "unavailable"; reason: UnavailableReason }
  | { kind: "external"; provider: string; observedAt: string; detail: string };

export type NetworkPolicy = { ruleDownload: boolean; passwordBreachCheck: boolean };
export type ProtectionSettings = { network: NetworkPolicy; amsiEnabled: boolean; allowlist: string[] };

export type RulesSource = "installed" | "previousFallback" | "embeddedBaseline";
export type RulesStatus = {
  source: RulesSource; sequence: number; created: string; description: string; ruleCount: number;
  previousSequence: number | null; recoveryNote: string | null; externalPacksAllowed: boolean;
};

export type HeuristicInfo = { id: string; title: string; falsePositiveNote: string };

export type ScanSummary = {
  filesScanned: number; deterministic: number; heuristic: number; unavailable: number;
  reparsePointsSkipped: number; allowlisted: number; truncated: boolean; cancelled: boolean; packSequence: number;
};
export type ScanScope = "quick" | "folder";
export type FindingView = {
  id: string; path: string; sha256: string | null; size: number | null; evidence: Evidence; canQuarantine: boolean;
};
export type ScanReport = { scope: ScanScope; summary: ScanSummary; findings: FindingView[]; finishedAt: string };
/** Backend scan state; counters are zero when no scan is running. */
export type ScanStatus = { running: boolean; filesScanned: number; bytesHashed: number };

export type ProtectionOverview = {
  rules: RulesStatus; settings: ProtectionSettings; quarantineCount: number;
  lastScan: ScanSummary | null; heuristics: HeuristicInfo[];
};

export type SignerStatus =
  | { state: "valid"; subject: string; microsoft: boolean }
  | { state: "unsigned" } | { state: "invalid" } | { state: "unavailable" } | { state: "notApplicable" };

export type ProcessEntry = {
  pid: number; parentPid: number; name: string; threadCount: number; imagePath: string | null;
  imageLocation: string | null; signer: SignerStatus; commandLine: Evidence; findings: Evidence[];
};
export type ProcessInventory = { processes: ProcessEntry[]; truncated: boolean; imageUnavailableCount: number };

export type QuarantineEntry = {
  id: string; originalPath: string | null; sha256: string | null; size: number | null;
  finding: string | null; quarantinedAt: number | null; damaged: boolean;
};

/**
 * Longest password (in UTF-8 bytes) the breach check accepts. Source of truth:
 * `MAX_PASSWORD_BYTES` in src-tauri/crates/windows-platform/src/protection/breach.rs.
 */
export const MAX_PASSWORD_BYTES = 1024;

export type PasswordBreachResult = { occurrences: number; evidence: Evidence };

export type DefenderDetection = { threatId: string | null; detectedAt: string | null; resources: string[]; evidence: Evidence };
export type DefenderHistory = { detections: DefenderDetection[]; unavailable: Evidence | null; truncated: boolean };

export type QuarantineErrorCode =
  | "invalidId" | "notFound" | "invalidPath" | "outsideRoot" | "reparsePoint" | "protected" | "inUse"
  | "accessDenied" | "tooLarge" | "changed" | "collision" | "hashMismatch" | "damaged" | "storage";
export type RuleRejection =
  | "badSignature" | "notNewer" | "requiresNewerApp" | "unsupportedFormat" | "invalid" | "tooLarge" | "noPrevious";
export type ProtectionErrorCode =
  | "busy" | "invalidInput" | "notFound" | "cancelled" | "storage" | "networkDisabled" | "networkFailed"
  | "rulePackMissing" | "rulesDisabled" | "confirmationDeclined" | "windowUnavailable"
  | { rulesRejected: RuleRejection }
  | { quarantine: QuarantineErrorCode };
export type ProtectionCommandError = { code: ProtectionErrorCode; message: string };
