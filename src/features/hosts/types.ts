export type HostsLineKind = "blank" | "comment" | "mapping" | "appDisabled" | "invalid";
export type HostsFindingSeverity = "low" | "high";
export type HostsFindingKind = "redirect" | "securityBlocked" | "customMapping" | "invalid";

export interface HostsFinding {
  /** Zero-based line index. */
  line: number;
  severity: HostsFindingSeverity;
  kind: HostsFindingKind;
  detail: string;
}

export interface HostsLineView {
  /** Zero-based line index. */
  index: number;
  kind: HostsLineKind;
  text: string;
}

export interface HostsReport {
  path: string;
  sha256: string;
  sizeBytes: number;
  totalLines: number;
  linesTruncated: boolean;
  lines: HostsLineView[];
  findings: HostsFinding[];
}
