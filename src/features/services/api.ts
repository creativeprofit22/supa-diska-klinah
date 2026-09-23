import { invoke } from "@tauri-apps/api/core";
import type { ServiceItem } from "./types";

export const listServices = () => invoke<ServiceItem[]>("list_services");
