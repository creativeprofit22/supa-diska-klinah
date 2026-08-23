import type { RouteObject } from "react-router-dom";
import { SettingsPage } from "./SettingsPage";

export const settingsRoute: RouteObject = {
  path: "settings",
  element: <SettingsPage />,
};
