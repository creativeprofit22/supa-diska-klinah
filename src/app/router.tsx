import { createHashRouter } from "react-router-dom";
import { dashboardRoute } from "../features/dashboard/route";
import { settingsRoute } from "../features/settings/route";
import { AppShell } from "../shared/layout/AppShell";

export const router = createHashRouter([
  {
    path: "/",
    element: <AppShell />,
    children: [dashboardRoute, settingsRoute],
  },
]);
