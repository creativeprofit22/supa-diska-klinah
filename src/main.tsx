import { createRoot } from "react-dom/client";
import { RouterProvider } from "react-router-dom";
import { router } from "./app/router";
import { AppSettingsProvider } from "./shared/app-settings/AppSettingsProvider";
import { I18nProvider } from "./shared/i18n/I18nProvider";
import "./styles.css";

const root = document.getElementById("root");

if (!root) {
  throw new Error("Application root element is missing");
}

createRoot(root).render(
  <I18nProvider>
    <AppSettingsProvider>
      <RouterProvider router={router} />
    </AppSettingsProvider>
  </I18nProvider>,
);
