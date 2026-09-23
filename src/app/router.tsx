import { uninstallerRoute } from "../features/uninstaller/route";
import { browserRoute } from "../features/browser/route";
import { cleanerRoute } from "../features/cleaner/route";
import { createHashRouter } from "react-router-dom";
import { cleanupRoute } from "../features/cleanup/route";
import { dashboardRoute } from "../features/dashboard/route";
import { drivesRoute } from "../features/drives/route";
import { diskAnalyzerRoute } from "../features/disk-analyzer/route";
import { duplicatesRoute } from "../features/duplicates/route";
import { emptyFoldersRoute } from "../features/empty-folders/route";
import { largeFilesRoute } from "../features/large-files/route";
import { settingsRoute } from "../features/settings/route";
import { AppShell } from "../shared/layout/AppShell";

export const router = createHashRouter([
  {
    path: "/",
    element: <AppShell />,
    children: [dashboardRoute, drivesRoute, diskAnalyzerRoute, largeFilesRoute, duplicatesRoute, emptyFoldersRoute, cleanerRoute, browserRoute, uninstallerRoute, cleanupRoute, settingsRoute],
  },
]);
