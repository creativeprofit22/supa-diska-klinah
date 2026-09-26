import { invoke } from "@tauri-apps/api/core";
import type { CleanerCatalog } from "./types";
export const listCleanerCatalog = () => invoke<CleanerCatalog>("list_cleaner_catalog", new TextEncoder().encode("{}"));
export const startCleaner = (rootId: string) => invoke<string>("start_cleaner", new TextEncoder().encode(JSON.stringify({ rootId })));
