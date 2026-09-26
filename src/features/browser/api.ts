import { invoke } from "@tauri-apps/api/core";
import type { BrowserPolicy } from "./types";
export const listBrowserPolicy = () => invoke<BrowserPolicy>("list_browser_policy", new TextEncoder().encode("{}"));
export const startBrowserScan = (rootId: string, serviceWorkerOptIn: boolean) => invoke<string>("start_browser_scan", new TextEncoder().encode(JSON.stringify({ rootId, serviceWorkerOptIn })));
