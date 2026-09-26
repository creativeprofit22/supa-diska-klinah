import { invoke } from "@tauri-apps/api/core";

export const UPDATE_ERROR_CODES = [
  "notConfigured", "notPermitted", "busy", "network", "badManifest", "noUpdateChecked", "downloadFailed",
  "integrityFailed", "signatureRejected", "storage", "nothingToInstall", "declined", "launchFailed",
] as const;
export type UpdateErrorCode = (typeof UPDATE_ERROR_CODES)[number];

export type UpdateState =
  /** Startup recovery is still re-checking a previous update; ask again shortly. */
  | { state: "recovering" }
  | { state: "idle" }
  | { state: "upToDate" }
  | { state: "available"; version: string; size: number }
  | { state: "downloading"; version: string }
  | { state: "verified"; version: string }
  | { state: "launched"; version: string }
  | { state: "interrupted"; version: string };

/** A one-time notice about what startup recovery found (never "clean"). */
export type UpdateRecovery =
  | { outcome: "updated"; version: string }
  | { outcome: "interrupted"; version: string }
  | { outcome: "discarded" };

export type UpdateStatus = Readonly<{
  currentVersion: string;
  configured: boolean;
  update: UpdateState;
  /** Present until the notice is resolved. */
  recovery?: UpdateRecovery;
  /** True while startup recovery has not finished yet. */
  recoveryPending?: true;
}>;

export type UpdateResult = { ok: true; value: UpdateStatus } | { ok: false; error: UpdateErrorCode | "invalidResponse" };

const VERSION = /^\d{1,9}\.\d{1,9}\.\d{1,9}$/;

function parseState(value: unknown): UpdateState | null {
  if (typeof value !== "object" || value === null) return null;
  const record = value as Record<string, unknown>;
  const version = typeof record.version === "string" && VERSION.test(record.version) ? record.version : null;
  switch (record.state) {
    case "recovering":
    case "idle":
    case "upToDate":
      return { state: record.state };
    case "available":
      return version && typeof record.size === "number" && Number.isSafeInteger(record.size) && record.size > 0
        ? { state: "available", version, size: record.size }
        : null;
    case "downloading":
    case "verified":
    case "launched":
    case "interrupted":
      return version ? { state: record.state, version } : null;
    default:
      return null;
  }
}

/** `undefined` = field absent, `null` = malformed. */
function parseRecovery(value: unknown): UpdateRecovery | undefined | null {
  if (value === undefined) return undefined;
  if (typeof value !== "object" || value === null) return null;
  const record = value as Record<string, unknown>;
  const keys = Object.keys(record);
  switch (record.outcome) {
    case "updated":
    case "interrupted":
      return keys.length === 2 && typeof record.version === "string" && VERSION.test(record.version)
        ? { outcome: record.outcome, version: record.version }
        : null;
    case "discarded":
      return keys.length === 1 ? { outcome: "discarded" } : null;
    default:
      return null;
  }
}

/** Validates the IPC response instead of trusting its declared type. */
export function parseUpdateStatus(value: unknown): UpdateStatus | null {
  if (typeof value !== "object" || value === null) return null;
  const record = value as Record<string, unknown>;
  const update = parseState(record.update);
  if (!update || typeof record.configured !== "boolean" || typeof record.currentVersion !== "string" || !VERSION.test(record.currentVersion)) {
    return null;
  }
  const recovery = parseRecovery(record.recovery);
  if (recovery === null || (record.recoveryPending !== undefined && record.recoveryPending !== true)) return null;
  return {
    currentVersion: record.currentVersion,
    configured: record.configured,
    update,
    ...(recovery ? { recovery } : {}),
    ...(record.recoveryPending === true ? { recoveryPending: true as const } : {}),
  };
}

function errorCode(reason: unknown): UpdateErrorCode | "invalidResponse" {
  return typeof reason === "string" && (UPDATE_ERROR_CODES as readonly string[]).includes(reason)
    ? (reason as UpdateErrorCode)
    : "invalidResponse";
}

async function call(command: string): Promise<UpdateResult> {
  try {
    const parsed = parseUpdateStatus(await invoke<unknown>(command));
    return parsed ? { ok: true, value: parsed } : { ok: false, error: "invalidResponse" };
  } catch (reason) {
    return { ok: false, error: errorCode(reason) };
  }
}

export const updateApi = {
  status: (): Promise<UpdateResult> => call("get_update_status"),
  check: (): Promise<UpdateResult> => call("check_for_update"),
  download: (): Promise<UpdateResult> => call("download_update"),
  install: (): Promise<UpdateResult> => call("install_update"),
  discard: (): Promise<UpdateResult> => call("discard_update"),
  /** Hides the startup recovery notice without touching a kept installer. */
  acknowledgeRecovery: (): Promise<UpdateResult> => call("acknowledge_update_recovery"),
};
export type UpdateApi = typeof updateApi;
