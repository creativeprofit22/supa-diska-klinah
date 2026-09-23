import { invoke } from "@tauri-apps/api/core";
import type { ExecutionReport, JournalView, PlanTicket, PlannedChange, SystemChange, SystemChangeError } from "./types";

export const MAX_PLAN_CHANGES = 32;

function request<T>(command: string, input: object): Promise<T> {
  return invoke<T>(command, new TextEncoder().encode(JSON.stringify(input)));
}

export const systemChangeActions = {
  preview: (change: SystemChange) => request<PlannedChange>("preview_system_change", { change }),
  createPlan: (changes: SystemChange[]) => request<PlanTicket>("create_system_change_plan", { changes }),
  confirm: (planId: string) => request<void>("confirm_system_change_plan", { planId }),
  execute: (planId: string) => request<ExecutionReport>("execute_system_change_plan", { planId }),
  journal: () => request<JournalView[]>("system_change_journal", {}),
  createRollbackPlan: (entryIds: string[]) => request<PlanTicket>("create_system_rollback_plan", { entryIds }),
};
export type SystemChangeActions = typeof systemChangeActions;

const messages: Record<string, string> = {
  confirmationDeclined: "You cancelled the Windows confirmation. Nothing was changed.",
  planExpired: "The review expired after 60 seconds. Review the changes again.",
  planNotFound: "This review is no longer valid. Review the changes again.",
  unsupported: "This change is not supported on this device.",
  busy: "Another system change is still running.",
  tooManyChanges: "Select at most 32 changes at a time.",
  journalUnavailable:
    "The change history could not be read. It may have been written by a newer version of this app; update the app, or see the administrator guide to reset it.",
};

export function systemChangeError(error: unknown): string {
  const code = (error as Partial<SystemChangeError> | null)?.code;
  if (typeof code === "string" && code in messages) return messages[code];
  const message = (error as Partial<SystemChangeError> | null)?.message;
  return typeof message === "string" && message.length <= 300 ? message : "The system change could not be completed.";
}
