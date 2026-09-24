import type { Evidence, ProtectionCommandError, SignerStatus, UnavailableReason } from "./types";

/**
 * Wording rule (ADR 0003): results describe what was observed and never
 * certify a file or device. The banned word list lives in
 * `scripts/check-architecture.mjs`, which enforces it for this folder.
 */
export const evidenceKindLabel: Record<Evidence["kind"], string> = {
  deterministic: "Signed-rule match",
  heuristic: "Heuristic",
  unavailable: "Not checked",
  external: "External response",
};

export const evidenceKindHint: Record<Evidence["kind"], string> = {
  deterministic: "Matched a hash or byte pattern in a signed rule pack.",
  heuristic: "A local pattern that is often suspicious. It can be a false positive.",
  unavailable: "This item could not be examined. That is not the same as no match.",
  external: "Reported by a component this app does not control.",
};

const unavailableLabel: Record<UnavailableReason, string> = {
  accessDenied: "Access denied",
  inUse: "In use by another program",
  tooLarge: "Larger than the scan limit",
  reparsePoint: "Link or junction, not followed",
  notFound: "No longer exists",
  signerNotCheckable: "Signature could not be checked",
  providerAbsent: "No provider available",
  providerNoResponse: "Provider gave no answer",
  cancelled: "Cancelled",
  readFailed: "Could not be read",
};

export function evidenceDetail(evidence: Evidence): string {
  switch (evidence.kind) {
    case "deterministic":
      return `${evidence.ruleName} (rule ${evidence.ruleId}, pack ${evidence.packSequence}, ${evidence.method === "sha256" ? "exact hash" : "byte pattern"})`;
    case "heuristic":
      return evidence.reason;
    case "unavailable":
      return unavailableLabel[evidence.reason];
    case "external":
      return `${evidence.provider}: ${evidence.detail}`;
  }
}

export function EvidenceBadge({ evidence }: { evidence: Evidence }) {
  return <span className={`evidence-badge evidence-${evidence.kind}`} title={evidenceKindHint[evidence.kind]}>
    {evidenceKindLabel[evidence.kind]}
  </span>;
}

export function signerLabel(signer: SignerStatus): string {
  switch (signer.state) {
    case "valid": return signer.subject ? `Signed: ${signer.subject}` : "Signed";
    case "unsigned": return "No embedded signature";
    case "invalid": return "Signature invalid";
    case "unavailable": return "Signature not checked";
    case "notApplicable": return "Not a signable file";
  }
}

export function errorMessage(reason: unknown, fallback: string): string {
  const error = reason as Partial<ProtectionCommandError> | null;
  return typeof error?.message === "string" && error.message.length <= 300 ? error.message : fallback;
}

export function isCollision(reason: unknown): boolean {
  const code = (reason as Partial<ProtectionCommandError> | null)?.code;
  return typeof code === "object" && code !== null && "quarantine" in code && code.quarantine === "collision";
}

export function isCancelled(reason: unknown): boolean {
  const code = (reason as Partial<ProtectionCommandError> | null)?.code;
  return code === "cancelled" || code === "confirmationDeclined";
}

/** Describes a scan result without implying the absence of threats. */
export function summaryLine(summary: { filesScanned: number; deterministic: number; heuristic: number; unavailable: number; packSequence: number }): string {
  const parts = [
    `${summary.filesScanned} files examined with rule pack ${summary.packSequence}`,
    summary.deterministic === 0 ? "no signed-rule matches" : `${summary.deterministic} signed-rule matches`,
    `${summary.heuristic} heuristic findings`,
    `${summary.unavailable} items not checked`,
  ];
  return parts.join(" · ");
}
