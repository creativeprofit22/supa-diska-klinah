// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { DashboardPage } from "./DashboardPage";

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
