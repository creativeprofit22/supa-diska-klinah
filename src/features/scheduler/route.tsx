import type { RouteObject } from "react-router-dom";
import { SchedulerPage } from "./SchedulerPage";
export const schedulerRoute: RouteObject = { path: "scheduled-scans", element: <SchedulerPage /> };
