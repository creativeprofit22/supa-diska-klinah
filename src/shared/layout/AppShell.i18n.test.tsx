// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { createMemoryRouter, RouterProvider } from "react-router-dom";
import { I18nProvider } from "../i18n/I18nProvider";
import { AppShell } from "./AppShell";

afterEach(cleanup);

it("renders navigation in Latin American Spanish and keeps the product name", () => {
  const router = createMemoryRouter([{ path: "/", element: <AppShell /> }]);
  render(<I18nProvider languages={["es-MX"]}><RouterProvider router={router} /></I18nProvider>);
  expect(screen.getByRole("navigation", { name: "Navegación principal" })).toBeTruthy();
  expect(screen.getByRole("link", { name: "Saltar al contenido" })).toBeTruthy();
  expect(screen.getByRole("link", { name: "Panel" })).toBeTruthy();
  expect(screen.getByRole("link", { name: "Configuración" })).toBeTruthy();
  expect(screen.getByText("Supa Diska Klinah")).toBeTruthy();
  expect(screen.getByText("Base para Windows")).toBeTruthy();
});
