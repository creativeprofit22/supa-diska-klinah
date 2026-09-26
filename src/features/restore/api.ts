import { invoke } from "@tauri-apps/api/core";
import type { RestorePointList, RestoreProtection } from "./types";

export const listRestorePoints = () => invoke<RestorePointList>("list_restore_points");
export const getRestoreProtection = () => invoke<RestoreProtection>("get_restore_protection");
