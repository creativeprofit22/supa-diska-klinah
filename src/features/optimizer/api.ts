import { invoke } from "@tauri-apps/api/core";
import type { OptimizerReport } from "./types";

export const getOptimizerProposals = () => invoke<OptimizerReport>("get_optimizer_proposals");
