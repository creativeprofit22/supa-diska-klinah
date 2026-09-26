// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { DashboardPage } from "./DashboardPage";
import { I18nProvider } from "../../shared/i18n/I18nProvider";

const invoke = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({ invoke }));

vi.mock("./model/useFoundationStatus", () => ({
  useFoundationStatus: () => ({
    status: { platform: "windows", architecture: "x86_64", adapterReady: true },
    error: null,
    loading: false,
    retry: vi.fn(),
  }),
}));

describe("Dashboard restore-point panel", () => {
  afterEach(() => {
    cleanup();
    invoke.mockReset();
  });

  it("links to the Restore points page instead of creating a restore point directly", () => {
    render(
      <MemoryRouter>
        <DashboardPage />
      </MemoryRouter>,
    );

    const link = screen.getByRole("link", { name: "Create or view restore points" });
    expect(link.getAttribute("href")).toBe("/restore-points");
    expect(screen.queryByRole("button", { name: "Create restore point" })).toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe("Dashboard localization", () => {
  afterEach(() => cleanup());
  it("renders Spanish copy for es-MX", () => {
    render(<I18nProvider languages={["es-MX"]}><MemoryRouter><DashboardPage /></MemoryRouter></I18nProvider>);
    expect(screen.getByRole("heading", { name: "Preparación del sistema" })).toBeTruthy();
    expect(screen.getByRole("link", { name: "Crear o ver puntos de restauración" })).toBeTruthy();
    expect(screen.getByText("Conectado")).toBeTruthy();
  });
});
