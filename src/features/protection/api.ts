import { invoke } from "@tauri-apps/api/core";
import type {
  DefenderHistory, NetworkPolicy, PasswordBreachResult, ProcessInventory, ProtectionOverview,
  ProtectionSettings, QuarantineEntry, RulesStatus, ScanReport, ScanScope, ScanStatus,
} from "./types";

/** Protection commands take raw JSON object bodies; the backend rejects anything else. */
function call<T>(command: string, input: object = {}): Promise<T> {
  return invoke<T>(command, new TextEncoder().encode(JSON.stringify(input)));
}

export const getOverview = () => call<ProtectionOverview>("protection_overview");
export const setNetworkPolicy = (network: NetworkPolicy, amsiEnabled: boolean) =>
  call<ProtectionSettings>("set_protection_network_policy", { network, amsiEnabled });
export const listProcesses = () => call<ProcessInventory>("list_running_programs");
export const startScan = (scope: ScanScope) => call<ScanReport>("start_protection_scan", { scope });
export const cancelScan = () => call<null>("cancel_protection_scan");
export const scanStatus = () => call<ScanStatus>("protection_scan_status");
export const lastScan = () => call<ScanReport | null>("last_protection_scan");
export const quarantineFinding = (id: string) => call<QuarantineEntry>("quarantine_protection_finding", { id });
export const listQuarantine = () => call<QuarantineEntry[]>("list_quarantine");
export const restoreQuarantined = (id: string) => call<string>("restore_quarantined", { id });
export const deleteQuarantined = (id: string) => call<null>("delete_quarantined", { id });
export const allowFinding = (id: string) => call<ProtectionSettings>("allow_protection_finding", { id });
export const clearAllowlist = () => call<ProtectionSettings>("clear_protection_allowlist");
export const importRulePack = () => call<RulesStatus>("import_rule_pack");
export const restorePreviousRulePack = () => call<RulesStatus>("restore_previous_rule_pack");
export const downloadRulePack = () => call<RulesStatus>("download_rule_pack");
/** Best-effort: zero the encoded body once the call settles. The JS string itself cannot be wiped. */
export async function checkPasswordBreach(password: string): Promise<PasswordBreachResult> {
  const body = new TextEncoder().encode(JSON.stringify({ password }));
  try {
    return await invoke<PasswordBreachResult>("check_password_breach", body);
  } finally {
    body.fill(0);
  }
}
export const getDefenderHistory = () => call<DefenderHistory>("get_defender_history");
