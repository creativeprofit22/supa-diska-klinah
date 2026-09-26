import { invoke } from "@tauri-apps/api/core";
import { systemChangeStrings, type SystemChangeStrings } from "./strings";
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

type ErrorStrings = SystemChangeStrings["errors"];

/** Maps a typed backend error code to localized text; free-form messages are shown only as detail. */
export function systemChangeError(error: unknown, t: ErrorStrings = systemChangeStrings.en.errors): string {
  const code = (error as Partial<SystemChangeError> | null)?.code;
  if (typeof code === "string" && code !== "fallback" && Object.hasOwn(t, code)) return t[code as keyof ErrorStrings];
  const message = (error as Partial<SystemChangeError> | null)?.message;
  return typeof message === "string" && message.length <= 300 ? message : t.fallback;
}
