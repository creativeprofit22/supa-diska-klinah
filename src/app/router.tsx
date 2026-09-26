import { uninstallerRoute } from "../features/uninstaller/route";
import { browserRoute } from "../features/browser/route";
import { cleanerRoute } from "../features/cleaner/route";
import { createHashRouter, type RouteObject } from "react-router-dom";
import { cleanupRoute } from "../features/cleanup/route";
import { dashboardRoute } from "../features/dashboard/route";
import { drivesRoute } from "../features/drives/route";
import { diskAnalyzerRoute } from "../features/disk-analyzer/route";
import { duplicatesRoute } from "../features/duplicates/route";
import { emptyFoldersRoute } from "../features/empty-folders/route";
import { largeFilesRoute } from "../features/large-files/route";
import { settingsRoute } from "../features/settings/route";
import { driversRoute } from "../features/drivers/route";
import { firewallRoute } from "../features/firewall/route";
import { hostsRoute } from "../features/hosts/route";
import { optimizerRoute } from "../features/optimizer/route";
import { powerRoute } from "../features/power/route";
import { privacyRoute } from "../features/privacy/route";
import { protectionRoute } from "../features/protection/route";
import { restoreRoute } from "../features/restore/route";
import { schedulerRoute } from "../features/scheduler/route";
import { servicesRoute } from "../features/services/route";
import { startupRoute } from "../features/startup/route";
import { updatesRoute } from "../features/updates/route";
import { AppShell } from "../shared/layout/AppShell";

export const appRoutes: RouteObject[] = [
  {
    path: "/",
    element: <AppShell />,
    children: [dashboardRoute, drivesRoute, diskAnalyzerRoute, largeFilesRoute, duplicatesRoute, emptyFoldersRoute, cleanerRoute, browserRoute, uninstallerRoute, cleanupRoute,
      optimizerRoute, startupRoute, servicesRoute, privacyRoute, firewallRoute, hostsRoute, powerRoute, driversRoute, restoreRoute, updatesRoute, schedulerRoute,
      protectionRoute, settingsRoute],
  },
];

export const router = createHashRouter(appRoutes);
