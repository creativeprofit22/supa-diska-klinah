import { invoke } from "@tauri-apps/api/core";
import type { PrivacyReport } from "./types";

export const getPrivacyReport = () => invoke<PrivacyReport>("get_privacy_report");
