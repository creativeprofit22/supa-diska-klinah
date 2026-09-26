import { invoke } from "@tauri-apps/api/core";
import type { StartupItem } from "./types";

export const listStartupItems = () => invoke<StartupItem[]>("list_startup_items");
