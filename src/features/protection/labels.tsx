import { useStrings } from "../../shared/i18n/I18nProvider";
import { protectionStrings, type ProtectionStrings } from "./strings";
import type { Evidence, ProtectionCommandError, SignerStatus } from "./types";

/**
 * Wording rule (ADR 0003): results describe what was observed and never
 * certify a file or device. The banned word list lives in
 * `scripts/check-architecture.mjs`, which enforces it for this folder.
 * Display text lives in `./strings.ts`.
 */
/** English labels; components use `useStrings(protectionStrings).evidenceKind`. */
export const evidenceKindLabel: Record<Evidence["kind"], string> = protectionStrings.en.evidenceKind;

export function evidenceDetail(evidence: Evidence, t: ProtectionStrings = protectionStrings.en): string {
  switch (evidence.kind) {
    case "deterministic":
      return t.deterministicDetail(evidence.ruleName, evidence.ruleId, String(evidence.packSequence), evidence.method === "sha256");
    case "heuristic":
      return evidence.reason;
    case "unavailable":
      return t.unavailable[evidence.reason];
    case "external":
      return `${evidence.provider}: ${evidence.detail}`;
  }
}

export function EvidenceBadge({ evidence }: { evidence: Evidence }) {
  const t = useStrings(protectionStrings);
  return <span className={`evidence-badge evidence-${evidence.kind}`} title={t.evidenceHint[evidence.kind]}>
    {t.evidenceKind[evidence.kind]}
  </span>;
}

export function signerLabel(signer: SignerStatus, t: ProtectionStrings = protectionStrings.en): string {
  switch (signer.state) {
    case "valid": return signer.subject ? t.signer.signedBy(signer.subject) : t.signer.signed;
    case "unsigned": return t.signer.unsigned;
    case "invalid": return t.signer.invalid;
    case "unavailable": return t.signer.unavailable;
    case "notApplicable": return t.signer.notApplicable;
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
export function summaryLine(
  summary: { filesScanned: number; deterministic: number; heuristic: number; unavailable: number; packSequence: number },
  t: ProtectionStrings = protectionStrings.en,
  number: (value: number) => string = String,
): string {
  const parts = [
    t.summary.examined(number(summary.filesScanned), String(summary.packSequence)),
    summary.deterministic === 0 ? t.summary.noSignedMatches : t.summary.signedMatches(number(summary.deterministic)),
    t.summary.heuristicFindings(number(summary.heuristic)),
    t.summary.notChecked(number(summary.unavailable)),
  ];
  return parts.join(" · ");
}
