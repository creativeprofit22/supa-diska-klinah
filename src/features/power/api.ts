import { invoke } from "@tauri-apps/api/core";
import type { PowerStatus } from "./types";

export const getPowerStatus = () => invoke<PowerStatus>("get_power_status");
