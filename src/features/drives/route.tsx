import type { RouteObject } from "react-router-dom";
import { DriveInventoryPage } from "./DriveInventoryPage";

export const drivesRoute: RouteObject = {
  path: "drives",
  element: <DriveInventoryPage />,
};
