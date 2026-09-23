import { invoke } from "@tauri-apps/api/core";
import type { DriverPackage } from "./types";

export const listDriverPackages = () => invoke<DriverPackage[]>("list_driver_packages");
