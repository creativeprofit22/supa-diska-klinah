import { useCallback, useEffect, useRef, useState } from "react";
import { systemChangeActions, systemChangeError, type SystemChangeActions } from "./api";
import { useStrings } from "../i18n/I18nProvider";
import { systemChangeStrings } from "./strings";
import type { ExecutionReport, PlanTicket, SystemChange } from "./types";

export type PlanPhase = "idle" | "planning" | "reviewing" | "applying" | "done";

/**
 * Drives one plan through review → native confirmation → execution. The
 * webview never confirms: `apply` asks the backend to show the Windows
 * dialog, and only then executes the opaque plan.
 */
export function useSystemChangePlan(actions: SystemChangeActions = systemChangeActions) {
  const [phase, setPhase] = useState<PlanPhase>("idle");
  const [ticket, setTicket] = useState<PlanTicket | null>(null);
  const [report, setReport] = useState<ExecutionReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const errors = useStrings(systemChangeStrings).errors;
  const mounted = useRef(true);
  const lock = useRef(false);
  useEffect(() => () => { mounted.current = false; }, []);

  const run = useCallback(async (work: () => Promise<void>) => {
    if (lock.current) return;
    lock.current = true;
    setError(null);
    try { await work(); } finally { lock.current = false; }
  }, []);

  const review = useCallback((create: () => Promise<PlanTicket>) => run(async () => {
    setPhase("planning"); setReport(null); setTicket(null);
    try {
      const next = await create();
      if (!mounted.current) return;
      setTicket(next); setPhase("reviewing");
    } catch (reason) {
      if (!mounted.current) return;
      setError(systemChangeError(reason, errors)); setPhase("idle");
    }
  }), [errors, run]);

  const reviewChanges = useCallback((changes: SystemChange[]) => review(() => actions.createPlan(changes)), [actions, review]);
  const reviewRollback = useCallback((entryIds: string[]) => review(() => actions.createRollbackPlan(entryIds)), [actions, review]);

  const apply = useCallback(() => run(async () => {
    if (!ticket) return;
    setPhase("applying");
    try {
      await actions.confirm(ticket.planId);
      const result = await actions.execute(ticket.planId);
      if (!mounted.current) return;
      setReport(result); setTicket(null); setPhase("done");
    } catch (reason) {
      if (!mounted.current) return;
      setError(systemChangeError(reason, errors)); setTicket(null); setPhase("idle");
    }
  }), [actions, errors, run, ticket]);

  const discard = useCallback(() => {
    if (lock.current) return;
    setTicket(null); setReport(null); setError(null); setPhase("idle");
  }, []);

  return { phase, ticket, report, error, reviewChanges, reviewRollback, apply, discard };
}
