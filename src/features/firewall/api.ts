import { invoke } from "@tauri-apps/api/core";
import type { FirewallStatus } from "./types";

export const getFirewallStatus = () => invoke<FirewallStatus>("get_firewall_status");
