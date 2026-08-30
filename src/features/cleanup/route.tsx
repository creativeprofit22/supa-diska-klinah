import type { RouteObject } from "react-router-dom";
import { CleanupPreviewPage } from "./CleanupPreviewPage";

export const cleanupRoute: RouteObject = {
  path: "cleanup",
  element: <CleanupPreviewPage />,
};
