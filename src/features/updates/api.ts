import { invoke } from "@tauri-apps/api/core";
import type { UpdateStatus } from "./types";

export const getWindowsUpdateStatus = () => invoke<UpdateStatus>("get_windows_update_status");
export const detectWindowsUpdates = () => invoke<void>("detect_windows_updates");
