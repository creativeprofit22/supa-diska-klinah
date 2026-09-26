import { invoke } from "@tauri-apps/api/core";
import type { HostsReport } from "./types";

export const getHostsReport = () => invoke<HostsReport>("get_hosts_report");
