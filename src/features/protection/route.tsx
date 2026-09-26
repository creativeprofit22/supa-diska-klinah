import type { RouteObject } from "react-router-dom";
import { BreachPage } from "./BreachPage";
import { OverviewPage } from "./OverviewPage";
import { ProcessesPage } from "./ProcessesPage";
import { ProtectionLayout } from "./ProtectionLayout";
import { QuarantinePage } from "./QuarantinePage";
import { RulesPage } from "./RulesPage";
import { ScanPage } from "./ScanPage";

export const protectionRoute: RouteObject = {
  path: "protection",
  element: <ProtectionLayout />,
  children: [
    { index: true, element: <OverviewPage /> },
    { path: "scan", element: <ScanPage /> },
    { path: "processes", element: <ProcessesPage /> },
    { path: "quarantine", element: <QuarantinePage /> },
    { path: "rules", element: <RulesPage /> },
    { path: "breach", element: <BreachPage /> },
  ],
};
